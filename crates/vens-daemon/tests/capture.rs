use vens_daemon::capture::Pdf;
use vens_emulator::network::FixtureNetwork;
use vens_net::{operation, trace};

const STACK: &str = include_str!("../../../fixtures/emulator/traces/three-duplex-sheets.trace");

fn object(pdf: &str, reference: u32) -> Option<&str> {
    let (_, start) = pdf.split_once(&format!("\n{reference} 0 obj\n"))?;
    start.split_once("\nendobj").map(|(object, _)| object)
}

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
        let objects = String::from_utf8_lossy(&pdf);
        let page_tree = object(&objects, 2)?;
        let ordered_pages = page_tree.contains("/Kids [3 0 R 6 0 R 9 0 R 12 0 R 15 0 R 18 0 R]")
            && page_tree.contains("/Count 6");
        let linked_images = [3_u32, 6, 9, 12, 15, 18].into_iter().all(|page| {
            let image = page.saturating_add(1);
            object(&objects, page)
                .is_some_and(|entry| entry.contains(&format!("/Scan {image} 0 R")))
                && object(&objects, image).is_some_and(|entry| entry.contains("/Filter /DCTDecode"))
        });
        Some((
            entries,
            images,
            pdf.starts_with(b"%PDF-"),
            ends,
            ordered_pages,
            linked_images,
        ))
    })();
    assert_eq!(result, Some((1, 6, true, true, true, true)));
}
