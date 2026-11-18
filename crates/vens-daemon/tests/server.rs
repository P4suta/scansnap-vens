//! The daemon serves whole eSCL jobs over HTTP on the loopback interface, with the emulator in place of the scanner.
//! Each connection carries one request, as sane-airscan sends them, and the server closes it after the answer.

use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    net::{Ipv4Addr, Shutdown, TcpListener, TcpStream},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use vens_core::jpeg::frame_size;
use vens_daemon::{
    http::Limits,
    server::{Config, Log, Networks, serve},
};
use vens_emulator::network::FixtureNetwork;
use vens_escl::uuid::Uuid;
use vens_net::trace::{self, Event};

const THREE_SHEETS: &str =
    include_str!("../../../fixtures/emulator/traces/three-duplex-sheets.trace");
const ONE_SHEET: &str = include_str!("../../../fixtures/emulator/traces/one-duplex-sheet.trace");
const EMPTY_CHUTE: &str = include_str!("../../../fixtures/emulator/traces/empty-chute.trace");
const RESERVE_REPLY: usize = 8;
const RESULT: usize = 8;
const SETTINGS: &str = r#"<?xml version="1.0" encoding="UTF-8"?><scan:ScanSettings xmlns:scan="http://schemas.hp.com/imaging/escl/2011/05/03" xmlns:pwg="http://www.pwg.org/schemas/2010/12/sm"><pwg:Version>2.0</pwg:Version><pwg:InputSource>Feeder</pwg:InputSource><scan:ColorMode>RGB24</scan:ColorMode><pwg:DocumentFormat>image/jpeg</pwg:DocumentFormat><scan:XResolution>300</scan:XResolution><scan:YResolution>300</scan:YResolution><scan:Duplex>true</scan:Duplex></scan:ScanSettings>"#;
const CONFIG: Config = Config {
    uuid: Uuid::from_bytes([0x11; 16]),
    seed: [0x22; 16],
    target: Ipv4Addr::BROADCAST,
    limits: Limits {
        head: 8192,
        body: 65536,
    },
    request_timeout: Duration::from_millis(300),
    write_timeout: Duration::from_secs(5),
};

struct Fixtures(VecDeque<Vec<Event>>);

impl Networks for Fixtures {
    type Network = FixtureNetwork;

    fn network(&mut self) -> Option<FixtureNetwork> {
        self.0.pop_front().and_then(FixtureNetwork::new)
    }
}

struct Lines(mpsc::Sender<String>);

impl Log for Lines {
    fn record(&mut self, line: &str) {
        let _ = self.0.send(line.to_owned());
    }
}

/// Starts a daemon on free loopback ports with one recorded session for each job, and returns its ports and its log.
fn start_on<const L: usize>(
    config: Config,
    sessions: Vec<Vec<Event>>,
) -> Option<([u16; L], mpsc::Receiver<String>)> {
    start_with(config, Fixtures(sessions.into()))
}

fn start_with<const L: usize>(
    config: Config,
    networks: impl Networks + Send + 'static,
) -> Option<([u16; L], mpsc::Receiver<String>)> {
    let listeners = [(); L].map(|()| TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok());
    let listeners: Vec<TcpListener> = listeners.into_iter().collect::<Option<_>>()?;
    let mut ports = [0; L];
    for (port, listener) in ports.iter_mut().zip(&listeners) {
        *port = listener.local_addr().ok()?.port();
    }
    let (lines, log) = mpsc::channel();
    thread::spawn(move || serve(&listeners, networks, config, Lines(lines)));
    Some((ports, log))
}

/// Starts a daemon on one free loopback port, and returns its port and its log.
fn start(sessions: Vec<Vec<Event>>) -> Option<(u16, mpsc::Receiver<String>)> {
    start_on::<1>(CONFIG, sessions).map(|([port], log)| (port, log))
}

fn events(fixture: &str) -> Vec<Event> {
    trace::parse(fixture).unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Answer {
    code: u16,
    fields: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Answer {
    fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    }

    fn says(&self, text: &str) -> bool {
        String::from_utf8_lossy(&self.body).contains(text)
    }
}

fn parse(bytes: &[u8]) -> Option<Answer> {
    let mut headers = [httparse::EMPTY_HEADER; 16];
    let mut response = httparse::Response::new(&mut headers);
    let httparse::Status::Complete(length) = response.parse(bytes).ok()? else {
        return None;
    };
    Some(Answer {
        code: response.code?,
        fields: response
            .headers
            .iter()
            .map(|header| {
                (
                    header.name.to_ascii_lowercase(),
                    String::from_utf8_lossy(header.value).into_owned(),
                )
            })
            .collect(),
        body: bytes.get(length..)?.to_vec(),
    })
}

fn exchange(port: u16, request: &[u8]) -> Option<Answer> {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .ok()?;
    stream.write_all(request).ok()?;
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).ok()?;
    parse(&bytes)
}

fn get(port: u16, path: &str) -> Option<Answer> {
    exchange(
        port,
        format!("GET {path} HTTP/1.1\r\nHost: bridge\r\nConnection: close\r\n\r\n").as_bytes(),
    )
}

fn post(port: u16, path: &str, body: &str) -> Option<Answer> {
    exchange(port, post_request(path, body).as_bytes())
}

fn post_request(path: &str, body: &str) -> String {
    format!(
        "POST {path} HTTP/1.1\r\nHost: bridge\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn delete(port: u16, path: &str) -> Option<Answer> {
    exchange(
        port,
        format!("DELETE {path} HTTP/1.1\r\nHost: bridge\r\nConnection: close\r\n\r\n").as_bytes(),
    )
}

#[test]
fn a_whole_duplex_job_runs_over_http() {
    let checks = start([events(THREE_SHEETS)].to_vec()).and_then(|(port, _log)| {
        let capabilities = get(port, "/eSCL/ScannerCapabilities")?;
        let created = post(port, "/eSCL/ScanJobs", SETTINGS)?;
        let location = created.field("location")?.to_owned();
        let next = format!("{location}/NextDocument");
        let pages = (0..6)
            .map(|_| {
                get(port, &next).map(|page| {
                    (
                        page.code,
                        page.field("content-type").map(str::to_owned),
                        frame_size(&page.body).is_ok(),
                    )
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let end = get(port, &next)?;
        let completed = get(port, "/eSCL/ScannerStatus")?;
        let deleted = delete(port, &location)?;
        let idle = get(port, "/eSCL/ScannerStatus")?;
        Some((
            (capabilities.code, capabilities.says("ScannerCapabilities")),
            created.code,
            pages,
            end.code,
            completed.says("<pwg:State>Idle</pwg:State>")
                && completed.says("ScannerAdfEmpty")
                && completed.says("<pwg:JobState>Completed</pwg:JobState>"),
            deleted.code,
            idle.says("<pwg:State>Idle</pwg:State>") && !idle.says("JobInfo"),
        ))
    });
    assert_eq!(
        checks,
        Some((
            (200, true),
            201,
            std::iter::repeat_n((200, Some("image/jpeg".to_owned()), true), 6).collect(),
            404,
            true,
            200,
            true
        ))
    );
}

#[test]
fn an_empty_chute_refuses_the_job_request_and_shows_in_the_status() {
    let checks = start([events(EMPTY_CHUTE)].to_vec()).and_then(|(port, _log)| {
        let refused = post(port, "/eSCL/ScanJobs", SETTINGS)?;
        let status = get(port, "/eSCL/ScannerStatus")?;
        Some((refused.code, status.says("ScannerAdfEmpty")))
    });
    assert_eq!(checks, Some((409, true)));
}

#[test]
fn a_scanner_held_by_another_host_answers_the_job_request_as_unavailable() {
    let mut occupied = events(ONE_SHEET);
    if let Some(reply) = occupied.get_mut(RESERVE_REPLY) {
        reply
            .bytes
            .iter_mut()
            .skip(RESULT)
            .zip((-4_i32).to_be_bytes())
            .for_each(|(slot, byte)| *slot = byte);
    }
    let refused = start([occupied].to_vec())
        .and_then(|(port, _log)| post(port, "/eSCL/ScanJobs", SETTINGS))
        .map(|answer| answer.code);
    assert_eq!(refused, Some(503));
}

#[test]
fn unknown_paths_wrong_methods_and_unreadable_requests_get_their_refusals() {
    let checks = start(Vec::new()).and_then(|(port, _log)| {
        Some((
            get(port, "/eSCL/ScannerIcon")?.code,
            post(port, "/eSCL/ScannerStatus", "")
                .map(|answer| (answer.code, answer.field("allow").map(str::to_owned)))?,
            exchange(port, b"GET / HTTP/9\r\n\r\n")?.code,
            post(port, "/eSCL/ScanJobs", "not a document")?.code,
            exchange(
                port,
                b"POST /eSCL/ScanJobs HTTP/1.1\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\n\r\n",
            )?
            .code,
        ))
    });
    assert_eq!(
        checks,
        Some((404, (405, Some("GET".to_owned())), 400, 400, 400))
    );
}

#[test]
fn a_silent_connection_times_out_while_other_clients_get_answers() {
    let checks = start(Vec::new()).and_then(|(port, _log)| {
        let mut silent = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).ok()?;
        silent
            .set_read_timeout(Some(Duration::from_secs(10)))
            .ok()?;
        let answered = get(port, "/eSCL/ScannerCapabilities")?.code;
        let mut rest = Vec::new();
        silent.read_to_end(&mut rest).ok()?;
        Some((answered, rest))
    });
    assert_eq!(checks, Some((200, Vec::new())));
}

#[test]
fn a_client_that_trickles_its_request_is_cut_off_at_the_request_deadline() {
    let checks = start(Vec::new()).and_then(|(port, _log)| {
        let mut reader = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).ok()?;
        reader
            .set_read_timeout(Some(Duration::from_secs(10)))
            .ok()?;
        let mut writer = reader.try_clone().ok()?;
        let started = Instant::now();
        thread::scope(|scope| {
            scope.spawn(move || {
                for byte in b"GET /eSCL/ScannerCapabilities HTTP/1.1\r\n\r\n" {
                    if writer.write_all(&[*byte]).is_err() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            });
            let mut answer = Vec::new();
            // A close with unread bytes in the server's buffer arrives as a reset, which ends the connection too.
            let closed = match reader.read_to_end(&mut answer) {
                Ok(_) => true,
                Err(error) => error.kind() == io::ErrorKind::ConnectionReset,
            };
            Some((
                closed && answer.is_empty(),
                started.elapsed() < Duration::from_secs(2),
            ))
        })
    });
    assert_eq!(checks, Some((true, true)));
}

#[test]
fn four_silent_connections_leave_an_acceptor_for_another_client() {
    let patient = Config {
        request_timeout: Duration::from_secs(3),
        ..CONFIG
    };
    let checks = start_on::<1>(patient, Vec::new()).and_then(|([port], _log)| {
        let silent: Vec<TcpStream> = (0..4)
            .map(|_| TcpStream::connect((Ipv4Addr::LOCALHOST, port)).ok())
            .collect::<Option<_>>()?;
        let started = Instant::now();
        let answered = get(port, "/eSCL/ScannerCapabilities")?.code;
        let elapsed = started.elapsed();
        drop(silent);
        Some((answered, elapsed < Duration::from_secs(1)))
    });
    assert_eq!(checks, Some((200, true)));
}

#[test]
fn a_job_that_times_out_before_reservation_gets_503_and_frees_the_bridge() {
    struct Gated {
        first: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
    }

    impl Networks for Gated {
        type Network = FixtureNetwork;

        fn network(&mut self) -> Option<FixtureNetwork> {
            if let Some((started, proceed)) = self.first.take() {
                started.send(()).ok()?;
                proceed.recv_timeout(Duration::from_secs(5)).ok()?;
            }
            FixtureNetwork::new(events(ONE_SHEET))
        }
    }

    let observed = (|| {
        let (started, queued) = mpsc::channel();
        let (proceed, hold) = mpsc::channel();
        let ([port], log) = start_with(
            Config {
                write_timeout: Duration::from_millis(300),
                ..CONFIG
            },
            Gated {
                first: Some((started, hold)),
            },
        )?;
        let mut client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).ok()?;
        client.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
        client
            .write_all(post_request("/eSCL/ScanJobs", SETTINGS).as_bytes())
            .ok()?;
        queued.recv_timeout(Duration::from_secs(5)).ok()?;
        let mut answer = Vec::new();
        client.read_to_end(&mut answer).ok()?;
        let timed_out = parse(&answer)?;
        proceed.send(()).ok()?;
        let lines = lines_until(&log, |line| line.contains("client left"));
        let next = post(port, "/eSCL/ScanJobs", SETTINGS)?;
        Some((
            (timed_out.code, timed_out.field("location").is_none()),
            lines
                .iter()
                .any(|line| line.contains("client left before the scan")),
            lines.iter().any(|line| line.contains("job reserved")),
            next.code,
        ))
    })();
    assert_eq!(observed, Some(((503, true), true, false, 201)));
}

#[test]
fn a_client_that_half_closes_after_its_job_request_gets_the_created_response() {
    let answer = start([events(ONE_SHEET)].to_vec()).and_then(|(port, _log)| {
        let mut client = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).ok()?;
        client.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
        client
            .write_all(post_request("/eSCL/ScanJobs", SETTINGS).as_bytes())
            .ok()?;
        client.shutdown(Shutdown::Write).ok()?;
        let mut bytes = Vec::new();
        client.read_to_end(&mut bytes).ok()?;
        let response = parse(&bytes)?;
        Some((response.code, response.field("location").is_some()))
    });
    assert_eq!(answer, Some((201, true)));
}

#[test]
fn each_listener_answers_requests() {
    let codes = start_on::<2>(CONFIG, Vec::new()).and_then(|(ports, _log)| {
        ports
            .iter()
            .map(|port| get(*port, "/eSCL/ScannerCapabilities").map(|answer| answer.code))
            .collect::<Option<Vec<_>>>()
    });
    assert_eq!(codes, Some([200, 200].to_vec()));
}

/// Reads log lines until one matches, and returns every line read.
fn lines_until(log: &mpsc::Receiver<String>, wanted: impl Fn(&str) -> bool) -> Vec<String> {
    let mut lines = Vec::new();
    while let Ok(line) = log.recv_timeout(Duration::from_secs(10)) {
        let found = wanted(&line);
        lines.push(line);
        if found {
            break;
        }
    }
    lines
}

#[test]
fn a_refused_job_request_logs_its_reason_client_and_body() {
    let body = SETTINGS.replace("RGB24", "Grayscale8");
    let lines = start(Vec::new()).and_then(|(port, log)| {
        exchange(
            port,
            format!(
                "POST /eSCL/ScanJobs HTTP/1.1\r\nHost: bridge\r\nUser-Agent: AirScanScanner/58\r\nContent-Type: text/xml\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )?;
        Some(lines_until(&log, |line| line.contains("Grayscale8")))
    });
    assert_eq!(
        lines.map(|lines| (
            lines.iter().any(|line| line.ends_with(
                "POST /eSCL/ScanJobs 409 Unsupported(ColorMode) client \"AirScanScanner/58\", type \"text/xml\""
            )),
            lines
                .iter()
                .any(|line| line.contains("POST /eSCL/ScanJobs body") && line.contains("Grayscale8")),
        )),
        Some((true, true))
    );
}

#[test]
fn an_accepted_job_request_logs_its_client_and_media_type() {
    let line = start([events(ONE_SHEET)].to_vec()).and_then(|(port, log)| {
        post(port, "/eSCL/ScanJobs", SETTINGS)?;
        lines_until(&log, |line| line.contains("POST /eSCL/ScanJobs 201")).pop()
    });
    assert_eq!(
        line.map(
            |line| line.ends_with("POST /eSCL/ScanJobs 201 client \"unnamed\", type \"text/xml\"")
        ),
        Some(true)
    );
}

#[test]
fn control_characters_in_a_request_target_reach_the_log_escaped() {
    let lines = start(Vec::new()).and_then(|(port, log)| {
        exchange(
            port,
            "GET /eSCL/\u{2028}\u{9d} HTTP/1.1\r\nHost: bridge\r\n\r\n".as_bytes(),
        )?;
        Some(lines_until(&log, |line| line.contains(" 404")))
    });
    assert_eq!(
        lines.map(|lines| lines.iter().any(|line| {
            line.ends_with("GET /eSCL/\\u{2028}\\u{9d} 404") && !line.contains('\u{2028}')
        })),
        Some(true)
    );
}

#[test]
fn each_page_logs_its_size_and_component_count() {
    let pages = start([events(ONE_SHEET)].to_vec()).and_then(|(port, log)| {
        let created = post(port, "/eSCL/ScanJobs", SETTINGS)?;
        let next = format!("{}/NextDocument", created.field("location")?);
        get(port, &next)?;
        get(port, &next)?;
        let lines = lines_until(&log, |line| line.starts_with("page Back"));
        Some(
            lines
                .into_iter()
                .filter(|line| line.starts_with("page "))
                .map(|line| line.ends_with(", components 1") && line.contains('x'))
                .collect::<Vec<_>>(),
        )
    });
    assert_eq!(pages, Some([true, true].to_vec()));
}

#[test]
fn the_log_names_each_request_with_its_status() {
    let line = start(Vec::new()).and_then(|(port, log)| {
        get(port, "/eSCL/ScannerCapabilities")?;
        log.recv_timeout(Duration::from_secs(10)).ok()
    });
    assert_eq!(
        line.map(|line| line.ends_with("GET /eSCL/ScannerCapabilities 200")),
        Some(true)
    );
}
