//! Each answer is one HTTP/1.1 response with a known length, and the daemon closes the connection after it.
//! RFC 9112, section 9.3, asks a server without persistent connections to say `close` in every response.

use vens_daemon::{
    framing::FramingError,
    http::{ReadError, Refusal, refusal, write_refusal, write_response},
};
use vens_escl::{
    bridge::{Conflict, Response, Unavailable},
    document::Capabilities,
    route::{JobPath, Method},
    uuid::Uuid,
};

const ID: Uuid = Uuid::from_bytes([0x11; 16]);

type Parsed = (u16, Vec<(String, String)>, Vec<u8>);

fn parse(bytes: &[u8]) -> Option<Parsed> {
    let mut headers = [httparse::EMPTY_HEADER; 16];
    let mut response = httparse::Response::new(&mut headers);
    let httparse::Status::Complete(length) = response.parse(bytes).ok()? else {
        return None;
    };
    let fields = response
        .headers
        .iter()
        .map(|header| {
            (
                header.name.to_ascii_lowercase(),
                String::from_utf8_lossy(header.value).into_owned(),
            )
        })
        .collect();
    Some((response.code?, fields, bytes.get(length..)?.to_vec()))
}

fn written(response: &Response<Vec<u8>>) -> Option<Parsed> {
    let mut bytes = Vec::new();
    write_response(&mut bytes, response).ok()?;
    parse(&bytes)
}

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn a_document_carries_its_type_and_length() {
    let body = Capabilities::new(ID).to_string();
    let length = body.len().to_string();
    assert_eq!(
        written(&Response::Capabilities(Capabilities::new(ID))),
        Some((
            200,
            fields(&[
                ("content-type", "text/xml"),
                ("content-length", &length),
                ("connection", "close"),
            ]),
            body.into_bytes()
        ))
    );
}

#[test]
fn a_page_carries_the_image_type_and_its_bytes() {
    let page = [0xFF, 0xD8, 0xFF, 0xD9].to_vec();
    assert_eq!(
        written(&Response::Page(page.clone())),
        Some((
            200,
            fields(&[
                ("content-type", "image/jpeg"),
                ("content-length", "4"),
                ("connection", "close"),
            ]),
            page
        ))
    );
}

#[test]
fn a_created_job_names_its_path_in_the_location() {
    let location = JobPath::new(ID).to_string();
    assert_eq!(
        written(&Response::Created(JobPath::new(ID))),
        Some((
            201,
            fields(&[
                ("location", &location),
                ("content-length", "0"),
                ("connection", "close"),
            ]),
            Vec::new()
        ))
    );
}

#[test]
fn a_method_refusal_names_the_allowed_method() {
    assert_eq!(
        written(&Response::MethodNotAllowed(Method::Post)),
        Some((
            405,
            fields(&[
                ("allow", "POST"),
                ("content-length", "0"),
                ("connection", "close"),
            ]),
            Vec::new()
        ))
    );
}

#[test]
fn each_answer_without_a_body_carries_its_status() {
    let codes = [
        Response::Deleted,
        Response::NotFound,
        Response::BadRequest,
        Response::Conflict(Conflict::NoPaper),
        Response::Unavailable(Unavailable::ScannerBusy),
    ]
    .map(|response| written(&response));
    let empty = |code| {
        Some((
            code,
            fields(&[("content-length", "0"), ("connection", "close")]),
            Vec::new(),
        ))
    };
    assert_eq!(codes, [200, 404, 400, 409, 503].map(empty));
}

#[test]
fn each_unreadable_request_gets_its_refusal() {
    let refusals = [
        ReadError::Malformed,
        ReadError::Framing(FramingError::Conflicting),
        ReadError::Framing(FramingError::InvalidLength),
        ReadError::Framing(FramingError::Unframed),
        ReadError::HeadTooLarge,
        ReadError::BodyTooLarge,
        ReadError::Framing(FramingError::UnsupportedCoding),
        ReadError::Closed,
        ReadError::Io(std::io::ErrorKind::TimedOut),
    ]
    .map(refusal);
    assert_eq!(
        refusals,
        [
            Some(Refusal::BadRequest),
            Some(Refusal::BadRequest),
            Some(Refusal::BadRequest),
            Some(Refusal::BadRequest),
            Some(Refusal::HeadTooLarge),
            Some(Refusal::BodyTooLarge),
            Some(Refusal::NotImplemented),
            None,
            None,
        ]
    );
}

#[test]
fn each_refusal_carries_its_status() {
    let codes = [
        Refusal::BadRequest,
        Refusal::HeadTooLarge,
        Refusal::BodyTooLarge,
        Refusal::NotImplemented,
    ]
    .map(|refusal| {
        let mut bytes = Vec::new();
        write_refusal(&mut bytes, refusal)
            .ok()
            .and_then(|()| parse(&bytes))
    });
    let empty = |code| {
        Some((
            code,
            fields(&[("content-length", "0"), ("connection", "close")]),
            Vec::new(),
        ))
    };
    assert_eq!(codes, [400, 431, 413, 501].map(empty));
}
