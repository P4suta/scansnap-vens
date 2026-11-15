//! Scans every sheet in the top chute once and writes one image file for each side.
//! Usage: `scan <output-directory> [scanner-address]`.
//! Load one sheet: the scanner feeds the whole stack, and this command sends no keepalive for a long job.

use std::{
    env, fmt, fs,
    io::{self, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    path::PathBuf,
    process::ExitCode,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use vens_core::{Ipv4Address, MacAddress, Port, control::LocalTime, discovery::Probe};
use vens_net::{
    channel::{ChannelError, CommandChannel},
    control::{ControlChannel, ControlError},
    host::{DirectorySink, SinkError, SleepingClock, TcpPort, UdpPort, local_address_toward},
    search::{SearchError, search},
    session::{Host, SessionError, scan_once},
};

const DISCOVERY_PORT: u16 = 52217;
const PROBE_INTERVAL: Duration = Duration::from_millis(500);
const COMMAND_DEADLINE: Duration = Duration::from_secs(30);
const ROUTE_PROBE: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 1);
const HOST_MAC: MacAddress = MacAddress::new([0x02, 0x76, 0x65, 0x6E, 0x73, 0x01]);
const REPLY_CAPACITY: usize = 0x0030_0040;

enum Failure {
    Usage,
    Address(String),
    Io(io::Error),
    Clock,
    Search(SearchError<io::Error>),
    Command(ChannelError<io::Error>),
    Control(ControlError<io::Error>),
    Session(SessionError<io::Error, SinkError>),
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => formatter
                .write_str("usage: scan <output-directory> [scanner-address]; load one sheet only"),
            Self::Address(text) => write!(formatter, "`{text}` is not an IPv4 address"),
            Self::Io(error) => write!(formatter, "input or output failure: {error}"),
            Self::Clock => formatter.write_str("the system clock is outside the supported range"),
            Self::Search(SearchError::Port(error)) => {
                write!(formatter, "discovery socket failure: {error}")
            }
            Self::Search(SearchError::NoReply { attempts }) => {
                write!(formatter, "no scanner answered {attempts} probes")
            }
            Self::Command(error) => write!(formatter, "command channel failure: {error:?}"),
            Self::Control(error) => write!(formatter, "control channel failure: {error:?}"),
            Self::Session(error) => write!(formatter, "scan session failure: {error:?}"),
        }
    }
}

fn arguments() -> Result<(PathBuf, Ipv4Addr), Failure> {
    let mut arguments = env::args().skip(1);
    let directory = arguments.next().map(PathBuf::from).ok_or(Failure::Usage)?;
    let target = arguments.next().map_or(Ok(Ipv4Addr::BROADCAST), |text| {
        text.parse().map_err(|_| Failure::Address(text))
    })?;
    Ok((directory, target))
}

fn now() -> Result<LocalTime, Failure> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| LocalTime::from_unix_seconds(elapsed.as_secs()))
        .ok_or(Failure::Clock)
}

fn trigger_port(socket: &UdpSocket) -> Result<Port, Failure> {
    match socket.local_addr().map_err(Failure::Io)? {
        SocketAddr::V4(address) => Ok(Port::new(address.port())),
        SocketAddr::V6(_) => Err(Failure::Io(io::Error::other(
            "the trigger socket isn't IPv4",
        ))),
    }
}

fn run(out: &mut impl Write) -> Result<(), Failure> {
    let (directory, target) = arguments()?;
    fs::create_dir_all(&directory).map_err(Failure::Io)?;
    let route = if target.is_broadcast() {
        ROUTE_PROBE
    } else {
        target
    };
    let local = local_address_toward(route).map_err(Failure::Io)?;
    let address = Ipv4Address::new(local.octets());
    let mut discovery = UdpPort::open(PROBE_INTERVAL).map_err(Failure::Io)?;
    let scanner = search(
        &mut discovery,
        Probe::search(address, HOST_MAC),
        SocketAddrV4::new(target, DISCOVERY_PORT),
    )
    .map_err(Failure::Search)?;
    let scanner_address = Ipv4Addr::from(scanner.address().octets());
    let triggers = UdpSocket::bind((local, 0)).map_err(Failure::Io)?;
    let host = Host {
        mac: HOST_MAC,
        address,
        trigger_port: trigger_port(&triggers)?,
        time: now()?,
    };
    let command_port = SocketAddrV4::new(scanner_address, scanner.command_port().number());
    let control_port = SocketAddrV4::new(scanner_address, scanner.control_port().number());
    let mut command = TcpPort::connect(command_port, COMMAND_DEADLINE)
        .map_err(Failure::Io)
        .and_then(|stream| {
            CommandChannel::open(stream, HOST_MAC, address).map_err(Failure::Command)
        })?;
    let mut control = TcpPort::connect(control_port, COMMAND_DEADLINE)
        .map_err(Failure::Io)
        .and_then(|stream| ControlChannel::open(stream).map_err(Failure::Control))?;
    let mut sink = DirectorySink::new(directory);
    let mut buffer = vec![0_u8; REPLY_CAPACITY];
    let summary = scan_once(
        &mut command,
        &mut control,
        host,
        &mut SleepingClock,
        &mut sink,
        &mut buffer,
    )
    .map_err(Failure::Session)?;
    writeln!(out, "sheets: {}", summary.sheets).map_err(Failure::Io)?;
    sink.written()
        .iter()
        .try_for_each(|path| writeln!(out, "{}", path.display()))
        .map_err(Failure::Io)
}

fn main() -> ExitCode {
    match run(&mut io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            let _ = writeln!(io::stderr().lock(), "scan failed: {failure}");
            ExitCode::FAILURE
        }
    }
}
