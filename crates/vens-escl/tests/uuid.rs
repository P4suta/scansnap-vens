//! Identifiers in the hyphenated form of RFC 9562, which eSCL uses for the scanner and its jobs.

use vens_escl::uuid::Uuid;

const TEXT: &str = "6f1c2e8a-3b4d-4c5e-9f60-7a8b9c0d1e2f";
const BYTES: [u8; 16] = [
    0x6f, 0x1c, 0x2e, 0x8a, 0x3b, 0x4d, 0x4c, 0x5e, 0x9f, 0x60, 0x7a, 0x8b, 0x9c, 0x0d, 0x1e, 0x2f,
];

#[test]
fn the_hyphenated_form_reads_into_bytes() {
    assert_eq!(Uuid::parse(TEXT), Some(Uuid::from_bytes(BYTES)));
}

#[test]
fn upper_case_digits_read_like_lower_case_ones() {
    assert_eq!(
        Uuid::parse("6F1C2E8A-3B4D-4C5E-9F60-7A8B9C0D1E2F"),
        Some(Uuid::from_bytes(BYTES))
    );
}

#[test]
fn an_identifier_renders_in_lower_case_with_hyphens() {
    assert_eq!(Uuid::from_bytes(BYTES).to_string(), TEXT);
}

#[test]
fn malformed_text_is_refused() {
    let malformed = [
        "",
        "6f1c2e8a-3b4d-4c5e-9f60-7a8b9c0d1e2",
        "6f1c2e8a-3b4d-4c5e-9f60-7a8b9c0d1e2f0",
        "6f1c2e8a3b4d-4c5e-9f60-7a8b9c0d1e2f0",
        "6f1c2e8a-3b4d-4c5e-9f607a8b-9c0d1e2f",
        "6f1c2e8a-3b4d-4c5e-9f60-7a8b9c0d1e2g",
        "{6f1c2e8a-3b4d-4c5e-9f60-7a8b9c0d1e}",
        "6f1c2e8a-3b4d-4c5e-9f60-7a8b9c0d1eé",
    ];
    assert_eq!(
        malformed.map(Uuid::parse),
        [None, None, None, None, None, None, None, None]
    );
}
