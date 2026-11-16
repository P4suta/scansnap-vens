//! The advertisement names the `_uscan._tcp` service with a TXT record that matches the capabilities.
//! sane-airscan reads `ty`, `uuid`, and `rs`, and Image Capture reads the rest of the keys that eSCL scanners publish.

use roxmltree::{Document, ParsingOptions};
use vens_daemon::advertise::{SERVICE_TYPE, avahi_service, txt_record};
use vens_escl::uuid::Uuid;

const ID: Uuid = Uuid::from_bytes([0x11; 16]);

fn expected_record() -> Vec<String> {
    [
        "txtvers=1",
        "vers=2.0",
        "rs=eSCL",
        "ty=ScanSnap iX1300",
        "uuid=11111111-1111-1111-1111-111111111111",
        "pdl=image/jpeg",
        "cs=color",
        "is=adf",
        "duplex=T",
    ]
    .map(str::to_owned)
    .to_vec()
}

#[test]
fn the_txt_record_starts_with_its_version_and_names_the_offer() {
    assert_eq!(txt_record(ID).to_vec(), expected_record());
}

#[test]
fn the_txt_record_fits_in_400_bytes_with_its_length_prefixes() {
    let size: usize = txt_record(ID)
        .iter()
        .map(|entry| entry.len().saturating_add(1))
        .sum();
    assert!(size < 400, "the record takes {size} bytes");
}

#[test]
fn the_avahi_service_advertises_the_type_port_and_record() {
    let xml = avahi_service(8090, ID);
    let options = ParsingOptions {
        allow_dtd: true,
        ..ParsingOptions::default()
    };
    let parsed = Document::parse_with_options(&xml, options)
        .ok()
        .map(|document| {
            let texts = |name: &str| {
                document
                    .descendants()
                    .filter(|node| node.has_tag_name(name))
                    .filter_map(|node| node.text())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            };
            (
                document.root_element().tag_name().name().to_owned(),
                texts("name"),
                texts("type"),
                texts("port"),
                texts("txt-record"),
            )
        });
    assert_eq!(
        parsed,
        Some((
            "service-group".to_owned(),
            ["ScanSnap iX1300 on %h".to_owned()].to_vec(),
            [SERVICE_TYPE.to_owned()].to_vec(),
            ["8090".to_owned()].to_vec(),
            expected_record(),
        ))
    );
}
