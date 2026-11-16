//! The daemon reads one request from each connection within fixed limits, because any host on the network can connect.
//! Every case reads its bytes in small pieces, as a slow client sends them.

use std::{
    collections::VecDeque,
    io::{self, Read, Write},
};

use vens_daemon::{
    framing::FramingError,
    http::{HttpRequest, Limits, ReadError, read_request},
};

const LIMITS: Limits = Limits {
    head: 256,
    body: 64,
};

struct Wire {
    incoming: VecDeque<u8>,
    piece: usize,
    failure: Option<io::ErrorKind>,
    outgoing: Vec<u8>,
}

impl Wire {
    fn new(incoming: &[u8], piece: usize) -> Self {
        Self {
            incoming: incoming.iter().copied().collect(),
            piece,
            failure: None,
            outgoing: Vec::new(),
        }
    }
}

impl Read for Wire {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.incoming.is_empty()
            && let Some(kind) = self.failure
        {
            return Err(io::Error::from(kind));
        }
        let mut count = 0_usize;
        for slot in buffer.iter_mut().take(self.piece) {
            match self.incoming.pop_front() {
                Some(byte) => {
                    *slot = byte;
                    count = count.saturating_add(1);
                }
                None => break,
            }
        }
        Ok(count)
    }
}

impl Write for Wire {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.outgoing.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn read(bytes: &[u8]) -> Result<HttpRequest, ReadError> {
    read_request(&mut Wire::new(bytes, 3), LIMITS)
}

fn request(method: &str, target: &str, body: &[u8]) -> HttpRequest {
    HttpRequest {
        method: method.to_owned(),
        target: target.to_owned(),
        body: body.to_vec(),
    }
}

#[test]
fn a_request_without_a_body_reads_its_method_and_target() {
    assert_eq!(
        read(b"GET /eSCL/ScannerStatus HTTP/1.1\r\nHost: scanner\r\nConnection: close\r\n\r\n"),
        Ok(request("GET", "/eSCL/ScannerStatus", b""))
    );
}

#[test]
fn a_body_with_a_length_reads_in_one_byte_pieces() {
    let mut wire = Wire::new(
        b"POST /eSCL/ScanJobs HTTP/1.1\r\nContent-Length: 11\r\n\r\n<settings/>",
        1,
    );
    assert_eq!(
        read_request(&mut wire, LIMITS),
        Ok(request("POST", "/eSCL/ScanJobs", b"<settings/>"))
    );
}

#[test]
fn a_chunked_body_decodes_across_chunks_and_trailers() {
    assert_eq!(
        read(
            b"POST /eSCL/ScanJobs HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n\
              4\r\n<set\r\n7;note=1\r\ntings/>\r\n0\r\nTrailer: value\r\n\r\n"
        ),
        Ok(request("POST", "/eSCL/ScanJobs", b"<settings/>"))
    );
}

#[test]
fn an_expectation_of_continue_gets_its_interim_answer_before_the_body() {
    let mut wire = Wire::new(
        b"POST /eSCL/ScanJobs HTTP/1.1\r\nExpect: 100-continue\r\nContent-Length: 2\r\n\r\nok",
        3,
    );
    let read = read_request(&mut wire, LIMITS);
    assert_eq!(
        (read, wire.outgoing),
        (
            Ok(request("POST", "/eSCL/ScanJobs", b"ok")),
            b"HTTP/1.1 100 Continue\r\n\r\n".to_vec()
        )
    );
}

#[test]
fn an_http_1_0_expectation_gets_no_interim_answer() {
    let mut wire = Wire::new(
        b"POST /eSCL/ScanJobs HTTP/1.0\r\nExpect: 100-continue\r\nContent-Length: 2\r\n\r\nok",
        3,
    );
    let read = read_request(&mut wire, LIMITS);
    assert_eq!(
        (read, wire.outgoing),
        (Ok(request("POST", "/eSCL/ScanJobs", b"ok")), Vec::new())
    );
}

#[test]
fn a_head_beyond_its_limit_is_refused() {
    let long_target = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(300));
    let many_headers = format!("GET / HTTP/1.1\r\n{}\r\n", "A: b\r\n".repeat(40));
    let headers_limits = Limits {
        head: 1024,
        body: 64,
    };
    assert_eq!(
        [
            read(long_target.as_bytes()),
            read_request(&mut Wire::new(many_headers.as_bytes(), 64), headers_limits),
        ],
        [Err(ReadError::HeadTooLarge), Err(ReadError::HeadTooLarge)]
    );
}

#[test]
fn a_body_beyond_its_limit_is_refused_before_it_arrives() {
    assert_eq!(
        read(b"POST /eSCL/ScanJobs HTTP/1.1\r\nContent-Length: 65\r\n\r\n"),
        Err(ReadError::BodyTooLarge)
    );
}

#[test]
fn a_chunked_body_beyond_its_limit_is_refused() {
    let chunk = format!(
        "POST /eSCL/ScanJobs HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n41\r\n{}\r\n0\r\n\r\n",
        "x".repeat(65)
    );
    assert_eq!(read(chunk.as_bytes()), Err(ReadError::BodyTooLarge));
}

#[test]
fn malformed_heads_and_chunk_sizes_are_refused() {
    assert_eq!(
        [
            read(b"GET /eSCL/ScannerStatus HTTP/9\r\n\r\n"),
            read(b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n"),
        ],
        [Err(ReadError::Malformed), Err(ReadError::Malformed)]
    );
}

#[test]
fn a_framing_error_carries_its_reason() {
    assert_eq!(
        read(b"POST / HTTP/1.1\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\n\r\nok"),
        Err(ReadError::Framing(FramingError::Conflicting))
    );
}

#[test]
fn a_connection_that_closes_early_is_closed() {
    assert_eq!(
        [
            read(b""),
            read(b"GET / HTTP/1.1\r\nHost"),
            read(b"POST / HTTP/1.1\r\nContent-Length: 4\r\n\r\nok"),
        ],
        [
            Err(ReadError::Closed),
            Err(ReadError::Closed),
            Err(ReadError::Closed)
        ]
    );
}

#[test]
fn a_read_that_times_out_reports_its_kind() {
    let mut wire = Wire::new(b"GET / HTTP/1.1\r\n", 3);
    wire.failure = Some(io::ErrorKind::WouldBlock);
    assert_eq!(
        read_request(&mut wire, LIMITS),
        Err(ReadError::Io(io::ErrorKind::WouldBlock))
    );
}
