//! Each subcommand runs on the emulator with a recorded fixture and produces the text that the user reads.
//! The busy and no-paper cases come from hardware recordings.
//! The occupied and unregistered cases change one recorded reply, because the scanner never showed them during the recordings.

use std::{env, fs, net::Ipv4Addr, path::PathBuf, process};

use vens_cli::{
    operation::{Scanned, discover, scan, status},
    report,
};
use vens_emulator::network::FixtureNetwork;
use vens_net::{
    host::DirectorySink,
    trace::{self, Event},
};

const THREE_SHEETS: &str =
    include_str!("../../../fixtures/emulator/traces/three-duplex-sheets.trace");
const EMPTY_CHUTE: &str = include_str!("../../../fixtures/emulator/traces/empty-chute.trace");
const STATUS_REPLY: usize = 6;
const RESERVE_REPLY: usize = 8;
const HOST_ID: usize = 0x20;
const RESERVE_RESULT: usize = 8;

const SCANNER: &str = "scanner: ScanSnap iX1300 (iX1300-EXAMPLE001)\naddress: 192.0.2.7\nports: command 53218, control 53219\ndevice status: 1\npassword: not set\n";

fn events(fixture: &str) -> Vec<Event> {
    trace::parse(fixture).unwrap_or_default()
}

fn network(events: Vec<Event>) -> Option<FixtureNetwork> {
    FixtureNetwork::new(events)
}

fn changed(fixture: &str, index: usize, offset: usize, bytes: &[u8]) -> Vec<Event> {
    let mut events = events(fixture);
    if let Some(event) = events.get_mut(index) {
        event
            .bytes
            .iter_mut()
            .skip(offset)
            .zip(bytes)
            .for_each(|(slot, byte)| *slot = *byte);
    }
    events
}

struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let path = env::temp_dir().join(format!("vens-cli-{}-{name}", process::id()));
        let _ = fs::remove_dir_all(&path);
        let _ = fs::create_dir_all(&path);
        Self(path)
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0).map_or_else(
            |_| Vec::new(),
            |entries| {
                entries
                    .filter_map(Result::ok)
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .collect()
            },
        );
        names.sort();
        names
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn discover_reports_the_recorded_scanner() {
    let text = network(events(THREE_SHEETS)).map(|mut network| {
        discover(&mut network, Ipv4Addr::BROADCAST).map(|reply| report::found(&reply))
    });
    assert_eq!(text, Some(Ok(SCANNER.to_owned())));
}

#[test]
fn status_reports_the_paper_path_and_the_registered_host() {
    let loaded = network(events(THREE_SHEETS)).map(|mut network| {
        status(&mut network, Ipv4Addr::BROADCAST).map(|state| report::state(&state))
    });
    let empty = network(events(EMPTY_CHUTE)).map(|mut network| {
        status(&mut network, Ipv4Addr::BROADCAST).map(|state| report::state(&state))
    });
    assert_eq!(
        (loaded, empty),
        (
            Some(Ok(format!(
                "{SCANNER}paper: top chute\nregistered host: present\n"
            ))),
            Some(Ok(format!(
                "{SCANNER}paper: none\nregistered host: present\n"
            ))),
        )
    );
}

#[test]
fn scan_writes_one_image_for_each_side_in_feed_order() {
    let folder = Folder::new("three-sheets");
    let mut sink = DirectorySink::new(folder.0.clone());
    let scanned = network(events(THREE_SHEETS))
        .map(|mut network| scan(&mut network, Ipv4Addr::BROADCAST, &mut sink));
    let written: Vec<PathBuf> = sink.written().to_vec();
    let feed_order: Vec<String> = ["001", "002", "003"]
        .iter()
        .flat_map(|sheet| {
            [
                format!("sheet-{sheet}-front.jpg"),
                format!("sheet-{sheet}-back.jpg"),
            ]
        })
        .collect();
    let mut on_disk = feed_order.clone();
    on_disk.sort();
    let written_names: Vec<String> = written
        .iter()
        .filter_map(|path| path.file_name()?.to_str().map(str::to_owned))
        .collect();
    let text = scanned
        .as_ref()
        .and_then(|outcome| outcome.as_ref().ok())
        .map(|scanned| report::scanned(scanned, &written));
    let mut lines = String::new();
    for path in &written {
        lines.push_str(&path.display().to_string());
        lines.push('\n');
    }
    assert_eq!(
        (scanned.map(Result::ok), written_names, folder.names(), text),
        (
            Some(Some(Scanned {
                sheets: 3,
                keepalive_replies: 3
            })),
            feed_order,
            on_disk,
            Some(format!("{lines}sheets: 3\nkeepalive replies: 3\n")),
        )
    );
}

#[test]
fn scan_of_an_empty_chute_says_so() {
    let folder = Folder::new("empty-chute");
    let mut sink = DirectorySink::new(folder.0.clone());
    let message = network(events(EMPTY_CHUTE)).and_then(|mut network| {
        scan(&mut network, Ipv4Addr::BROADCAST, &mut sink)
            .err()
            .map(|failure| report::failure(&failure))
    });
    assert_eq!(
        (message, folder.names()),
        (Some("no paper in the top chute".to_owned()), Vec::new())
    );
}

#[test]
fn a_scanner_held_by_another_computer_is_explained() {
    let folder = Folder::new("occupied");
    let mut sink = DirectorySink::new(folder.0.clone());
    let occupied = changed(
        EMPTY_CHUTE,
        RESERVE_REPLY,
        RESERVE_RESULT,
        &(-4_i32).to_be_bytes(),
    );
    let message = network(occupied).and_then(|mut network| {
        scan(&mut network, Ipv4Addr::BROADCAST, &mut sink)
            .err()
            .map(|failure| report::failure(&failure))
    });
    assert_eq!(
        message,
        Some("another computer holds the scanner; quit ScanSnap Home there, or close and open the scanner cover".to_owned())
    );
}

#[test]
fn a_scanner_without_a_registered_host_is_explained() {
    let folder = Folder::new("unregistered");
    let mut sink = DirectorySink::new(folder.0.clone());
    let unregistered = changed(EMPTY_CHUTE, STATUS_REPLY, HOST_ID, &[0xFF; 8]);
    let message = network(unregistered).and_then(|mut network| {
        scan(&mut network, Ipv4Addr::BROADCAST, &mut sink)
            .err()
            .map(|failure| report::failure(&failure))
    });
    assert_eq!(
        message,
        Some("the scanner has no registered host; pair it once with ScanSnap Home".to_owned())
    );
}
