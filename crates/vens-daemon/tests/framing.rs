//! The length of a request body follows the order of RFC 9112, section 6.3.
//! A request that frames its body two ways, or with an unknown length, is refused, because a wrong guess would misread the next bytes.

use vens_daemon::framing::{Framing, FramingError, framing};

fn frame(pairs: &[(&str, &str)]) -> Result<Framing, FramingError> {
    framing(pairs.iter().map(|(name, value)| (*name, value.as_bytes())))
}

#[test]
fn a_content_length_gives_the_body_length() {
    assert_eq!(
        frame(&[("Host", "scanner"), ("Content-Length", "42")]),
        Ok(Framing::Length(42))
    );
}

#[test]
fn header_names_match_in_any_letter_case() {
    assert_eq!(
        [
            frame(&[("content-length", "7")]),
            frame(&[("TRANSFER-ENCODING", "chunked")]),
        ],
        [Ok(Framing::Length(7)), Ok(Framing::Chunked)]
    );
}

#[test]
fn repeated_equal_lengths_count_once() {
    assert_eq!(
        [
            frame(&[("Content-Length", "42, 42")]),
            frame(&[("Content-Length", "42"), ("Content-Length", "42")]),
        ],
        [Ok(Framing::Length(42)), Ok(Framing::Length(42))]
    );
}

#[test]
fn a_request_without_framing_headers_has_no_body() {
    assert_eq!(
        [frame(&[]), frame(&[("Host", "scanner")])],
        [Ok(Framing::Length(0)), Ok(Framing::Length(0))]
    );
}

#[test]
fn the_chunked_coding_frames_the_body_in_any_letter_case() {
    assert_eq!(
        [
            frame(&[("Transfer-Encoding", "chunked")]),
            frame(&[("Transfer-Encoding", "Chunked")]),
        ],
        [Ok(Framing::Chunked), Ok(Framing::Chunked)]
    );
}

#[test]
fn malformed_or_differing_lengths_are_refused() {
    let refused = [
        "42, 43",
        "abc",
        "",
        "-1",
        "+5",
        "4 2",
        "0x10",
        "18446744073709551616",
    ]
    .map(|length| frame(&[("Content-Length", length)]));
    assert_eq!(refused, [Err(FramingError::InvalidLength); 8]);
}

#[test]
fn lengths_that_differ_across_fields_are_refused() {
    assert_eq!(
        frame(&[("Content-Length", "42"), ("Content-Length", "43")]),
        Err(FramingError::InvalidLength)
    );
}

#[test]
fn a_body_framed_two_ways_is_refused() {
    assert_eq!(
        frame(&[("Content-Length", "42"), ("Transfer-Encoding", "chunked")]),
        Err(FramingError::Conflicting)
    );
}

#[test]
fn a_coding_list_that_doesnt_end_in_one_chunked_coding_leaves_the_body_unframed() {
    let refused = ["chunked, gzip", "gzip", "chunked, chunked"]
        .map(|codings| frame(&[("Transfer-Encoding", codings)]));
    assert_eq!(refused, [Err(FramingError::Unframed); 3]);
}

#[test]
fn another_coding_before_chunked_is_unsupported() {
    assert_eq!(
        [
            frame(&[("Transfer-Encoding", "gzip, chunked")]),
            frame(&[
                ("Transfer-Encoding", "gzip"),
                ("Transfer-Encoding", "chunked")
            ]),
        ],
        [
            Err(FramingError::UnsupportedCoding),
            Err(FramingError::UnsupportedCoding)
        ]
    );
}
