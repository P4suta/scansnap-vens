use std::io::{self, Read, Write};

use httparse::{EMPTY_HEADER, Status};

use crate::framing::{Framing, FramingError, framing};

const MAX_HEADERS: usize = 32;
const PIECE: usize = 4096;
const LINE_END: &[u8] = b"\r\n";
const CONTINUE: &[u8] = b"HTTP/1.1 100 Continue\r\n\r\n";

/// The largest head and body that the daemon reads from one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub head: usize,
    pub body: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub target: String,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadError {
    /// The connection failed or timed out.
    Io(io::ErrorKind),
    /// The client closed the connection before the request ended.
    Closed,
    Malformed,
    HeadTooLarge,
    BodyTooLarge,
    Framing(FramingError),
}

struct Head {
    method: String,
    target: String,
    framing: Framing,
    expects_continue: bool,
    length: usize,
}

fn parse_head(bytes: &[u8]) -> Result<Option<Head>, ReadError> {
    let mut headers = [EMPTY_HEADER; MAX_HEADERS];
    let mut request = httparse::Request::new(&mut headers);
    let length = match request.parse(bytes) {
        Ok(Status::Complete(length)) => length,
        Ok(Status::Partial) => return Ok(None),
        Err(httparse::Error::TooManyHeaders) => return Err(ReadError::HeadTooLarge),
        Err(
            httparse::Error::HeaderName
            | httparse::Error::HeaderValue
            | httparse::Error::NewLine
            | httparse::Error::Status
            | httparse::Error::Token
            | httparse::Error::Version,
        ) => return Err(ReadError::Malformed),
    };
    let fields = request
        .headers
        .iter()
        .map(|header| (header.name, header.value));
    let framing = framing(fields).map_err(ReadError::Framing)?;
    let expects_continue = request.version == Some(1)
        && request.headers.iter().any(|header| {
            header.name.eq_ignore_ascii_case("expect")
                && header.value.eq_ignore_ascii_case(b"100-continue")
        });
    let (Some(method), Some(target)) = (request.method, request.path) else {
        return Err(ReadError::Malformed);
    };
    Ok(Some(Head {
        method: method.to_owned(),
        target: target.to_owned(),
        framing,
        expects_continue,
        length,
    }))
}

struct Incoming<'a, S> {
    stream: &'a mut S,
    pending: Vec<u8>,
}

impl<S: Read + Write> Incoming<'_, S> {
    fn fill(&mut self) -> Result<(), ReadError> {
        let mut piece = [0_u8; PIECE];
        let count = loop {
            match self.stream.read(&mut piece) {
                Ok(count) => break count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(ReadError::Io(error.kind())),
            }
        };
        if count == 0 {
            return Err(ReadError::Closed);
        }
        self.pending.extend(piece.iter().take(count));
        Ok(())
    }

    fn take(&mut self, length: usize) -> Vec<u8> {
        let rest = self.pending.split_off(length.min(self.pending.len()));
        core::mem::replace(&mut self.pending, rest)
    }

    fn head(&mut self, limit: usize) -> Result<Head, ReadError> {
        loop {
            if let Some(head) = parse_head(&self.pending)? {
                if head.length > limit {
                    return Err(ReadError::HeadTooLarge);
                }
                self.take(head.length);
                return Ok(head);
            }
            if self.pending.len() >= limit {
                return Err(ReadError::HeadTooLarge);
            }
            self.fill()?;
        }
    }

    fn exact(&mut self, length: u64, limit: usize) -> Result<Vec<u8>, ReadError> {
        let length = usize::try_from(length)
            .ok()
            .filter(|length| *length <= limit)
            .ok_or(ReadError::BodyTooLarge)?;
        while self.pending.len() < length {
            self.fill()?;
        }
        Ok(self.take(length))
    }

    fn chunk_size(&mut self, limit: usize) -> Result<u64, ReadError> {
        loop {
            match httparse::parse_chunk_size(&self.pending) {
                Ok(Status::Complete((consumed, size))) => {
                    self.take(consumed);
                    return Ok(size);
                }
                Ok(Status::Partial) if self.pending.len() < limit => self.fill()?,
                Ok(Status::Partial) | Err(httparse::InvalidChunkSize) => {
                    return Err(ReadError::Malformed);
                }
            }
        }
    }

    fn trailers(&mut self, limit: usize) -> Result<(), ReadError> {
        let mut read = 0_usize;
        loop {
            let line_end = self
                .pending
                .windows(LINE_END.len())
                .position(|pair| pair == LINE_END);
            match line_end {
                Some(0) => {
                    self.take(LINE_END.len());
                    return Ok(());
                }
                Some(end) => {
                    let line = self.take(end.saturating_add(LINE_END.len()));
                    read = read.saturating_add(line.len());
                    if read > limit {
                        return Err(ReadError::HeadTooLarge);
                    }
                }
                None if self.pending.len() >= limit => return Err(ReadError::HeadTooLarge),
                None => self.fill()?,
            }
        }
    }

    fn chunked(&mut self, limits: Limits) -> Result<Vec<u8>, ReadError> {
        let mut body = Vec::new();
        loop {
            let size = self.chunk_size(limits.head)?;
            if size == 0 {
                self.trailers(limits.head)?;
                return Ok(body);
            }
            let size = usize::try_from(size)
                .ok()
                .filter(|size| {
                    body.len()
                        .checked_add(*size)
                        .is_some_and(|total| total <= limits.body)
                })
                .ok_or(ReadError::BodyTooLarge)?;
            while self.pending.len() < size.saturating_add(LINE_END.len()) {
                self.fill()?;
            }
            body.append(&mut self.take(size));
            if self.take(LINE_END.len()) != LINE_END {
                return Err(ReadError::Malformed);
            }
        }
    }
}

/// Reads one request, its head within `limits.head` bytes and its body within `limits.body` bytes.
///
/// A request that expects to continue gets the interim answer before its body.
///
/// # Errors
///
/// Fails when the connection fails, times out, or closes early, and when the request is malformed, too large, or framed ambiguously.
pub fn read_request<S: Read + Write>(
    stream: &mut S,
    limits: Limits,
) -> Result<HttpRequest, ReadError> {
    let mut incoming = Incoming {
        stream,
        pending: Vec::new(),
    };
    let head = incoming.head(limits.head)?;
    if head.expects_continue && head.framing != Framing::Length(0) {
        incoming
            .stream
            .write_all(CONTINUE)
            .and_then(|()| incoming.stream.flush())
            .map_err(|error| ReadError::Io(error.kind()))?;
    }
    let body = match head.framing {
        Framing::Length(length) => incoming.exact(length, limits.body)?,
        Framing::Chunked => incoming.chunked(limits)?,
    };
    Ok(HttpRequest {
        method: head.method,
        target: head.target,
        body,
    })
}
