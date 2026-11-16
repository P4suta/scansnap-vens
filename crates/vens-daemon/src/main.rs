use std::{
    env,
    hash::{BuildHasher, RandomState},
    io::{self, Write},
    net::{Ipv4Addr, Ipv6Addr, TcpListener},
    process::ExitCode,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use vens_core::control::LocalTime;
use vens_daemon::{
    advertise::avahi_service,
    http::Limits,
    invocation::{self, Invocation, USAGE},
    server::{self, Config, Log, Networks},
};
use vens_escl::uuid::Uuid;
use vens_net::host::SystemNetwork;

const PROBE_INTERVAL: Duration = Duration::from_millis(500);
const DEADLINE: Duration = Duration::from_secs(30);
const LIMITS: Limits = Limits {
    head: 16_384,
    body: 8_192,
};
/// Gives each request this long to arrive in full; eSCL requests are a few kilobytes at most.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// Leaves room for a page of several megabytes over a slow wireless link.
const WRITE_TIMEOUT: Duration = Duration::from_secs(60);
const USAGE_FAILURE: u8 = 2;

struct System;

impl Networks for System {
    type Network = SystemNetwork;

    fn network(&mut self) -> Option<SystemNetwork> {
        Some(SystemNetwork::new(PROBE_INTERVAL, DEADLINE))
    }
}

/// Draws the job identifier seed from the randomly keyed hasher that the standard library seeds from the system.
fn seed() -> [u8; 16] {
    let keys = RandomState::new();
    let [high, low] = [0_u8, 1].map(|input| keys.hash_one(input).to_be_bytes());
    let mut seed = [0; 16];
    for (slot, byte) in seed.iter_mut().zip(high.into_iter().chain(low)) {
        *slot = byte;
    }
    seed
}

fn since_epoch() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

fn timestamp() -> String {
    LocalTime::from_unix_seconds(since_epoch().as_secs()).map_or_else(String::new, |time| {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            time.year(),
            time.month(),
            time.day(),
            time.hour(),
            time.minute(),
            time.second()
        )
    })
}

struct Stderr;

impl Log for Stderr {
    fn record(&mut self, line: &str) {
        let _ = writeln!(io::stderr().lock(), "{} vensd: {line}", timestamp());
    }
}

/// Listens on both address families where the host allows it, and on IPv4 alone otherwise.
fn bind(port: u16) -> io::Result<TcpListener> {
    TcpListener::bind((Ipv6Addr::UNSPECIFIED, port))
        .or_else(|_| TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)))
}

fn serve(port: u16, target: Ipv4Addr, uuid: Uuid) -> ExitCode {
    let mut log = Stderr;
    let listener = match bind(port) {
        Ok(listener) => listener,
        Err(error) => {
            log.record(&format!("can't listen on port {port}: {error}"));
            return ExitCode::FAILURE;
        }
    };
    let local = listener
        .local_addr()
        .map_or_else(|_| format!("port {port}"), |address| address.to_string());
    log.record(&format!(
        "listening on {local} as {uuid}, scanner at {target}"
    ));
    let config = Config {
        uuid,
        seed: seed(),
        target,
        limits: LIMITS,
        request_timeout: REQUEST_TIMEOUT,
        write_timeout: WRITE_TIMEOUT,
    };
    server::serve(&listener, System, config, log);
    ExitCode::FAILURE
}

fn print(text: &str) -> ExitCode {
    let mut stdout = io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

fn main() -> ExitCode {
    match invocation::parse(env::args_os().skip(1)) {
        Ok(Invocation::Serve { port, target, uuid }) => serve(port, target, uuid),
        Ok(Invocation::AvahiService { port, uuid }) => print(&avahi_service(port, uuid)),
        Ok(Invocation::Help) => print(USAGE),
        Err(error) => {
            let _ = write!(io::stderr().lock(), "vensd: {error}\n{USAGE}");
            ExitCode::from(USAGE_FAILURE)
        }
    }
}
