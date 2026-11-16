//! Each subcommand runs on the emulator with a recorded fixture and produces what the user sees.
//! The busy, keepalive, and no-paper cases come from hardware recordings.
//! The occupied, unregistered, jam, refused release, and silent cases change recorded replies, because the scanner never showed them during the recordings.

use std::{
    env, fs,
    net::{Ipv4Addr, SocketAddrV4},
    path::{Path, PathBuf},
    process,
};

use vens_cli::{
    command::{Exit, Output, run, scan_output},
    invocation::Invocation,
};
use vens_emulator::network::{Contact, FixtureNetwork};
use vens_net::{
    network::Connection,
    operation::{ScanRun, Scanned},
    trace::{self, Channel, Direction, Event},
};

const THREE_SHEETS: &str =
    include_str!("../../../fixtures/emulator/traces/three-duplex-sheets.trace");
const ONE_SHEET: &str = include_str!("../../../fixtures/emulator/traces/one-duplex-sheet.trace");
const EMPTY_CHUTE: &str = include_str!("../../../fixtures/emulator/traces/empty-chute.trace");
const STATUS_REPLY: usize = 6;
const RESERVE_REPLY: usize = 8;
const LAST_SENSE: usize = 158;
const RELEASE_REPLY: usize = 162;
const HOST_ID: usize = 0x20;
const RESULT: usize = 8;
const QUALIFIER: usize = 13;
const SUBFOLDER: &str = "20260101-000000";
const SCANNER: &str = "scanner: ScanSnap iX1300 (iX1300-EXAMPLE001)\naddress: 192.0.2.7\nports: command 53218, control 53219\ndevice status: 1\npassword: not set\n";

fn events(fixture: &str) -> Vec<Event> {
    trace::parse(fixture).unwrap_or_default()
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

fn invoke(events: Vec<Event>, invocation: Invocation) -> Option<Output> {
    FixtureNetwork::new(events).map(|network| run(network, invocation))
}

fn contacts(events: Vec<Event>, invocation: Invocation) -> Option<(Output, Vec<Contact>)> {
    FixtureNetwork::observed(events)
        .map(|(network, log)| (run(network, invocation), log.try_iter().collect()))
}

fn scanner(port: u16) -> SocketAddrV4 {
    SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 7), port)
}

fn success(stdout: &str) -> Output {
    Output {
        stdout: stdout.to_owned(),
        stderr: String::new(),
        exit: Exit::Success,
    }
}

fn failure(stderr: &str) -> Output {
    Output {
        stdout: String::new(),
        stderr: stderr.to_owned(),
        exit: Exit::Failure,
    }
}

struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let path = env::temp_dir().join(format!("vens-cli-{}-{name}", process::id()));
        let _ = fs::remove_dir_all(&path);
        let _ = fs::create_dir_all(&path);
        Self(path)
    }

    fn scan(&self) -> Invocation {
        Invocation::Scan {
            directory: self.0.clone(),
            target: Ipv4Addr::BROADCAST,
            trace: None,
        }
    }

    fn subfolder(&self) -> PathBuf {
        self.0.join(SUBFOLDER)
    }

    fn names(path: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(path).map_or_else(
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

fn images(folder: &Path, sheets: u32) -> String {
    let mut lines = String::new();
    for sheet in 1..=sheets {
        for side in ["front", "back"] {
            let path = folder.join(format!("sheet-{sheet:03}-{side}.jpg"));
            lines.push_str(&path.display().to_string());
            lines.push('\n');
        }
    }
    lines
}

#[test]
fn discover_reports_the_recorded_scanner() {
    assert_eq!(
        invoke(
            events(THREE_SHEETS),
            Invocation::Discover {
                target: Ipv4Addr::BROADCAST
            }
        ),
        Some(success(SCANNER))
    );
}

#[test]
fn discover_marks_a_busy_scanner_and_a_password() {
    const SEARCH_REPLY: usize = 1;
    let busy = changed(THREE_SHEETS, SEARCH_REPLY, 38, &[0x00, 0x02]);
    let protected = changed(THREE_SHEETS, SEARCH_REPLY, 12, &[0x00, 0x00, 0x00, 0x01]);
    let discover = |events| {
        invoke(
            events,
            Invocation::Discover {
                target: Ipv4Addr::BROADCAST,
            },
        )
        .map(|output| output.stdout)
    };
    assert_eq!(
        (discover(busy), discover(protected)),
        (
            Some(SCANNER.replace("device status: 1", "device status: 2 (busy)")),
            Some(SCANNER.replace(
                "password: not set",
                "password: required, which vens doesn't support yet"
            )),
        )
    );
}

#[test]
fn discover_probes_the_target_that_it_is_given() {
    let broadcast = contacts(
        events(THREE_SHEETS),
        Invocation::Discover {
            target: Ipv4Addr::BROADCAST,
        },
    )
    .map(|(_, contacts)| contacts);
    let unicast = contacts(
        events(THREE_SHEETS),
        Invocation::Discover {
            target: Ipv4Addr::new(192, 0, 2, 7),
        },
    )
    .map(|(_, contacts)| contacts);
    assert_eq!(
        (broadcast, unicast),
        (
            Some(
                [Contact::Datagram(SocketAddrV4::new(
                    Ipv4Addr::BROADCAST,
                    52217
                ))]
                .to_vec()
            ),
            Some([Contact::Datagram(scanner(52217))].to_vec()),
        )
    );
}

#[test]
fn status_reports_the_paper_path_and_the_registered_host() {
    let status = |events| {
        invoke(
            events,
            Invocation::Status {
                target: Ipv4Addr::BROADCAST,
            },
        )
    };
    assert_eq!(
        (
            status(events(THREE_SHEETS)),
            status(events(EMPTY_CHUTE)),
            status(changed(EMPTY_CHUTE, STATUS_REPLY, 2, &[0x80])),
            status(changed(EMPTY_CHUTE, STATUS_REPLY, HOST_ID, &[0xFF; 8])),
        ),
        (
            Some(success(&format!(
                "{SCANNER}paper: top chute\nregistered host: present\n"
            ))),
            Some(success(&format!(
                "{SCANNER}paper: none\nregistered host: present\n"
            ))),
            Some(success(&format!(
                "{SCANNER}paper: front slot\nregistered host: present\n"
            ))),
            Some(success(&format!(
                "{SCANNER}paper: none\nregistered host: absent\n"
            ))),
        )
    );
}

#[test]
fn status_opens_the_command_connection_alone() {
    assert_eq!(
        contacts(
            events(EMPTY_CHUTE),
            Invocation::Status {
                target: Ipv4Addr::BROADCAST
            }
        )
        .map(|(_, contacts)| contacts),
        Some(
            [
                Contact::Datagram(SocketAddrV4::new(Ipv4Addr::BROADCAST, 52217)),
                Contact::Connect(scanner(53218), Connection::Command),
            ]
            .to_vec()
        )
    );
}

#[test]
fn scan_writes_one_image_for_each_side_in_feed_order_into_a_new_subfolder() {
    let folder = Folder::new("three-sheets");
    let output = invoke(events(THREE_SHEETS), folder.scan());
    assert_eq!(
        (output, Folder::names(&folder.subfolder()).len()),
        (
            Some(success(&format!(
                "{}sheets: 3\nkeepalive replies: 3\n",
                images(&folder.subfolder(), 3)
            ))),
            6
        )
    );
}

#[test]
fn scan_searches_connects_and_keeps_the_reserve_alive_at_the_recorded_endpoints() {
    let folder = Folder::new("contacts");
    assert_eq!(
        contacts(events(THREE_SHEETS), folder.scan()).map(|(_, contacts)| contacts),
        Some(
            [
                Contact::Datagram(SocketAddrV4::new(Ipv4Addr::BROADCAST, 52217)),
                Contact::Connect(scanner(53218), Connection::Command),
                Contact::Connect(scanner(53219), Connection::Control),
                Contact::Datagram(scanner(52217)),
            ]
            .to_vec()
        )
    );
}

#[test]
fn a_second_scan_in_the_same_second_never_touches_the_first_one() {
    let folder = Folder::new("twice");
    let first = invoke(events(THREE_SHEETS), folder.scan()).map(|output| output.exit);
    let before = Folder::names(&folder.subfolder());
    let second = invoke(events(THREE_SHEETS), folder.scan());
    let expected_prefix = format!(
        "vens: the folder {} couldn't be created: ",
        folder.subfolder().display()
    );
    assert_eq!(
        (
            first,
            second.as_ref().map(|output| (
                output.stdout.clone(),
                output.stderr.starts_with(&expected_prefix),
                output.exit
            )),
            Folder::names(&folder.subfolder()) == before,
        ),
        (
            Some(Exit::Success),
            Some((String::new(), true, Exit::Failure)),
            true
        )
    );
}

#[test]
fn a_trace_keeps_every_frame_of_each_channel_in_order() {
    let folder = Folder::new("traced");
    let path = folder.0.join("raw.trace");
    let output = invoke(
        events(THREE_SHEETS),
        Invocation::Scan {
            directory: folder.0.clone(),
            target: Ipv4Addr::BROADCAST,
            trace: Some(path.clone()),
        },
    );
    let recorded = fs::read_to_string(&path)
        .ok()
        .and_then(|text| trace::parse(&text).ok())
        .unwrap_or_default();
    let channel = |events: &[Event], channel: Channel| -> Vec<(Direction, Vec<u8>)> {
        events
            .iter()
            .filter(|event| event.channel == channel)
            .map(|event| (event.direction, event.bytes.clone()))
            .collect()
    };
    let fixture = events(THREE_SHEETS);
    assert_eq!(
        (
            output.map(|output| output.exit),
            channel(&recorded, Channel::Discovery) == channel(&fixture, Channel::Discovery),
            channel(&recorded, Channel::Command) == channel(&fixture, Channel::Command),
            channel(&recorded, Channel::Control) == channel(&fixture, Channel::Control),
        ),
        (Some(Exit::Success), true, true, true)
    );
}

#[test]
fn a_trace_that_cannot_be_created_stops_the_scan_before_it_starts() {
    let folder = Folder::new("trace-missing");
    let path = folder.0.join("missing").join("raw.trace");
    let output = invoke(
        events(THREE_SHEETS),
        Invocation::Scan {
            directory: folder.0.clone(),
            target: Ipv4Addr::BROADCAST,
            trace: Some(path.clone()),
        },
    );
    let expected_prefix = format!("vens: the trace {} couldn't be created: ", path.display());
    assert_eq!(
        (
            output.map(|output| (output.stderr.starts_with(&expected_prefix), output.exit)),
            Folder::names(&folder.0),
        ),
        (Some((true, Exit::Failure)), Vec::new())
    );
}

#[test]
fn scan_of_an_empty_chute_says_so_and_leaves_no_folder() {
    let folder = Folder::new("empty-chute");
    assert_eq!(
        (
            invoke(events(EMPTY_CHUTE), folder.scan()),
            Folder::names(&folder.0)
        ),
        (
            Some(failure(
                "vens: no paper in the top chute; load the stack and scan again\n"
            )),
            Vec::new()
        )
    );
}

#[test]
fn a_scanner_held_by_another_computer_is_explained() {
    let folder = Folder::new("occupied");
    let occupied = changed(EMPTY_CHUTE, RESERVE_REPLY, RESULT, &(-4_i32).to_be_bytes());
    assert_eq!(
        invoke(occupied, folder.scan()),
        Some(failure(
            "vens: another computer holds the scanner; quit ScanSnap Home there, or close and open the scanner cover\n"
        ))
    );
}

#[test]
fn a_scanner_without_a_registered_host_is_explained() {
    let folder = Folder::new("unregistered");
    let unregistered = changed(EMPTY_CHUTE, STATUS_REPLY, HOST_ID, &[0xFF; 8]);
    assert_eq!(
        invoke(unregistered, folder.scan()),
        Some(failure(
            "vens: the scanner has no registered host; pair it once with ScanSnap Home\n"
        ))
    );
}

#[test]
fn a_failure_after_saved_images_lists_them_and_says_where_they_stay() {
    let folder = Folder::new("jam");
    let jammed = changed(THREE_SHEETS, LAST_SENSE, QUALIFIER, &[0x01]);
    let refused = changed(THREE_SHEETS, RELEASE_REPLY, RESULT, &(-1_i32).to_be_bytes());
    let jam = invoke(jammed, folder.scan());
    let _ = fs::remove_dir_all(folder.subfolder());
    let release = invoke(refused, folder.scan());
    let stays = format!(
        "vens: the images above stay in {}; scan the remaining sheets again\n",
        folder.subfolder().display()
    );
    assert_eq!(
        (jam, release),
        (
            Some(Output {
                stdout: images(&folder.subfolder(), 3),
                stderr: format!(
                    "vens: paper jam; open the scanner, remove the paper, and scan the remaining sheets again\n{stays}"
                ),
                exit: Exit::Failure,
            }),
            Some(Output {
                stdout: images(&folder.subfolder(), 3),
                stderr: format!(
                    "vens: 3 sheets scanned, but the scanner refused the release with result -1\n{stays}"
                ),
                exit: Exit::Failure,
            }),
        )
    );
}

#[test]
fn a_keepalive_failure_is_shown_and_never_stops_the_scan() {
    let folder = Folder::new("keepalive");
    assert_eq!(
        invoke(events(ONE_SHEET), folder.scan()),
        Some(Output {
            stdout: format!(
                "{}sheets: 1\nkeepalive replies: 0\n",
                images(&folder.subfolder(), 1)
            ),
            stderr: "vens: the keepalive failed: the recording has ended\n".to_owned(),
            exit: Exit::Success,
        })
    );
}

fn silent() -> Vec<Event> {
    let recorded = events(THREE_SHEETS);
    let mut discovery = recorded
        .iter()
        .filter(|event| event.channel == Channel::Discovery)
        .take(2);
    let probe = discovery.next().cloned();
    let reply = discovery.next().cloned();
    std::iter::repeat_n(probe, 21)
        .chain([reply])
        .flatten()
        .collect()
}

#[test]
fn no_reply_names_the_next_step_for_a_broadcast_and_for_an_address() {
    let discover = |target| invoke(silent(), Invocation::Discover { target });
    assert_eq!(
        (
            discover(Ipv4Addr::BROADCAST),
            discover(Ipv4Addr::new(192, 0, 2, 7)),
        ),
        (
            Some(failure(
                "vens: no scanner answered 20 search probes; check that the scanner is on and on this network, give its address with --address, and let vens receive replies through the firewall\n"
            )),
            Some(failure(
                "vens: no scanner answered 20 search probes at 192.0.2.7; check that the scanner is on and still has that address\n"
            )),
        )
    );
}

#[test]
fn a_scan_that_feeds_no_sheet_fails() {
    let run: ScanRun<String, _> = ScanRun {
        outcome: Ok(Scanned {
            sheets: 0,
            keepalive_replies: 1,
        }),
        keepalive_failure: None,
    };
    assert_eq!(
        scan_output(&run, &[], Path::new("scans"), None),
        failure("vens: the scanner fed no sheet; check that the stack sits in the top chute\n")
    );
}
