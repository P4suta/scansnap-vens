//! Finds the scanner and reads its hardware status without reserving it.
//! The output leaves out the serial number and the host identifier.

use std::{
    env, fmt,
    io::{self, Write},
    net::{Ipv4Addr, SocketAddrV4},
    process::ExitCode,
    time::Duration,
};

use vens_core::{
    Ipv4Address, MacAddress,
    command::ScsiStatus,
    discovery::{Availability, Password, Probe, SearchReply},
    scan::Command,
    status::{HardwareStatus, PaperPath, StatusError},
};
use vens_net::{
    channel::{ChannelError, CommandChannel},
    host::{TcpPort, UdpPort, local_address_toward},
    search::{SearchError, search},
};

const DISCOVERY_PORT: u16 = 52217;
const PROBE_INTERVAL: Duration = Duration::from_millis(500);
const COMMAND_DEADLINE: Duration = Duration::from_secs(5);
const ROUTE_PROBE: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 1);
const PROBE_MAC: MacAddress = MacAddress::new([0x02, 0x76, 0x65, 0x6E, 0x73, 0x01]);
const STATUS_CAPACITY: usize = 64;

enum Failure {
    Usage(String),
    Io(io::Error),
    Search(SearchError<io::Error>),
    Channel(ChannelError<io::Error>),
    Refused(ScsiStatus),
    Status(StatusError),
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(text) => write!(formatter, "`{text}` is not an IPv4 address"),
            Self::Io(error) => write!(formatter, "socket failure: {error}"),
            Self::Search(SearchError::Port(error)) => {
                write!(formatter, "discovery socket failure: {error}")
            }
            Self::Search(SearchError::NoReply { attempts }) => {
                write!(formatter, "no scanner answered {attempts} probes")
            }
            Self::Channel(error) => write!(formatter, "command channel failure: {error:?}"),
            Self::Refused(status) => {
                write!(formatter, "the hardware status command returned {status:?}")
            }
            Self::Status(error) => write!(formatter, "unreadable hardware status: {error:?}"),
        }
    }
}

fn target() -> Result<Ipv4Addr, Failure> {
    env::args().nth(1).map_or(Ok(Ipv4Addr::BROADCAST), |text| {
        text.parse().map_err(|_| Failure::Usage(text))
    })
}

const fn password(reply: &SearchReply) -> &'static str {
    match reply.password() {
        Password::NotSet => "not set",
        Password::Required => "required",
    }
}

const fn availability(reply: &SearchReply) -> &'static str {
    match reply.availability() {
        Availability::Ready => "ready",
        Availability::Busy => "busy",
        Availability::Unrecognized(_) => "unrecognized",
    }
}

const fn paper(status: &HardwareStatus) -> &'static str {
    match status.paper() {
        PaperPath::TopChute => "top chute",
        PaperPath::FrontSlot => "front slot",
        PaperPath::Absent => "absent",
    }
}

fn registration(reply: &SearchReply, status: &HardwareStatus) -> &'static str {
    match status.registered_host() {
        None => "absent",
        Some(host) if host == reply.last_owner() => "present, same as the last owner",
        Some(_) => "present, differs from the last owner",
    }
}

fn report(
    out: &mut impl Write,
    reply: &SearchReply,
    status: &HardwareStatus,
) -> Result<(), io::Error> {
    let [first, second, third, fourth] = reply.address().octets();
    writeln!(
        out,
        "product: {}",
        reply.product_name().as_str().unwrap_or("unreadable")
    )?;
    writeln!(out, "address: {first}.{second}.{third}.{fourth}")?;
    writeln!(out, "command port: {}", reply.command_port().number())?;
    writeln!(out, "control port: {}", reply.control_port().number())?;
    writeln!(out, "password: {}", password(reply))?;
    writeln!(out, "availability: {}", availability(reply))?;
    writeln!(out, "paper: {}", paper(status))?;
    writeln!(
        out,
        "exit sensor: {}",
        if status.exit_sensor() { "set" } else { "clear" }
    )?;
    writeln!(
        out,
        "registered host identifier: {}",
        registration(reply, status)
    )
}

fn run(out: &mut impl Write) -> Result<(), Failure> {
    let target = target()?;
    let route = if target.is_broadcast() {
        ROUTE_PROBE
    } else {
        target
    };
    let host = local_address_toward(route)
        .map(|address| Ipv4Address::new(address.octets()))
        .map_err(Failure::Io)?;
    let mut port = UdpPort::open(PROBE_INTERVAL).map_err(Failure::Io)?;
    let reply = search(
        &mut port,
        Probe::search(host, PROBE_MAC),
        SocketAddrV4::new(target, DISCOVERY_PORT),
    )
    .map_err(Failure::Search)?;
    let scanner = SocketAddrV4::new(
        Ipv4Addr::from(reply.address().octets()),
        reply.command_port().number(),
    );
    let stream = TcpPort::connect(scanner, COMMAND_DEADLINE).map_err(Failure::Io)?;
    let mut channel = CommandChannel::open(stream, PROBE_MAC, host).map_err(Failure::Channel)?;
    let mut buffer = [0; STATUS_CAPACITY];
    let answer = channel
        .execute(&Command::hardware_status(), &mut buffer)
        .map_err(Failure::Channel)?;
    if answer.status != ScsiStatus::Good {
        return Err(Failure::Refused(answer.status));
    }
    let status = HardwareStatus::decode(answer.data).map_err(Failure::Status)?;
    report(out, &reply, &status).map_err(Failure::Io)
}

fn main() -> ExitCode {
    match run(&mut io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            let _ = writeln!(io::stderr().lock(), "probe failed: {failure}");
            ExitCode::FAILURE
        }
    }
}
