use vens_daemon::capture::Pdf;
use vens_emulator::network::FixtureNetwork;
use vens_net::{operation, trace};

const STACK: &str = include_str!("../../../fixtures/emulator/traces/three-duplex-sheets.trace");

#[test]
fn a_recorded_stack_publishes_one_pdf_with_six_embedded_jpegs() {
    let result = (|| {
        let output = tempfile::tempdir().ok()?;
        let mut network = FixtureNetwork::new(trace::parse(STACK).ok()?)?;
        let mut pdf = Pdf::default();
        operation::scan(&mut network, std::net::Ipv4Addr::BROADCAST, &mut pdf)
            .outcome
            .ok()?;
        let published = pdf.publish(output.path()).ok()?;
        let pdf = std::fs::read(published).ok()?;
        let entries = std::fs::read_dir(output.path()).ok()?.count();
        let images = pdf
            .windows(2)
            .filter(|bytes| *bytes == [0xFF, 0xD8])
            .count();
        let ends = pdf.strip_suffix(b"\n").unwrap_or(&pdf).ends_with(b"%%EOF");
        Some((entries, images, pdf.starts_with(b"%PDF-"), ends))
    })();
    assert_eq!(result, Some((1, 6, true, true)));
}
