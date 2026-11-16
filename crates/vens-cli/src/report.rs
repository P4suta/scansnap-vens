use std::{
    fmt::Display,
    net::Ipv4Addr,
    path::{Path, PathBuf},
};

use vens_core::{
    command::{ReplyHeaderError, ScsiStatus},
    control::{GreetingError, ReserveRefusal},
    discovery::{Availability, Password, SearchReply},
    job::{JobError, Pause},
    sense::Condition,
    status::PaperPath,
};
use vens_net::{
    channel::ChannelError,
    control::ControlError,
    network::Connection,
    operation::{Failure, Scanned, State},
    scan::ScanError,
    session::SessionError,
};

use crate::invocation::UsageError;

const USAGE: &str = "usage: vens <subcommand> [options]

  vens discover [--address <ipv4>]
      find the scanner and show its identity
  vens status [--address <ipv4>]
      show the identity, the paper path, and the registered host
  vens scan <folder> [--address <ipv4>] [--trace <file>]
      scan the stack in the top chute into a new subfolder of the folder,
      one image for each side
  vens help
      show this text

Without --address, the search goes to the broadcast address.
A trace file receives every frame with its identifiers and image bytes.
";

#[must_use]
pub const fn usage() -> &'static str {
    USAGE
}

#[must_use]
pub fn usage_error(error: &UsageError) -> String {
    match error {
        UsageError::MissingSubcommand => "a subcommand is missing".to_owned(),
        UsageError::UnknownSubcommand(name) => format!("unknown subcommand `{name}`"),
        UsageError::MissingDirectory => "scan needs a folder".to_owned(),
        UsageError::MissingValue(option) => format!("{option} needs a value"),
        UsageError::InvalidAddress(text) => format!("`{text}` is not an IPv4 address"),
        UsageError::UnknownOption(option) => format!("unknown option `{option}`"),
        UsageError::ForeignOption { option, subcommand } => {
            format!("{subcommand} doesn't take `{option}`")
        }
        UsageError::Unexpected(argument) => format!("unexpected argument `{argument}`"),
    }
}

fn text(field: Option<&str>) -> &str {
    field.unwrap_or("(not text)")
}

/// Shows the identity and the endpoints of a scanner from its search reply.
#[must_use]
pub fn found(reply: &SearchReply) -> String {
    let device_status = match reply.availability() {
        Availability::Ready => "0".to_owned(),
        Availability::Busy => "2 (busy)".to_owned(),
        Availability::Unrecognized(code) => code.to_string(),
    };
    let password = match reply.password() {
        Password::NotSet => "not set",
        Password::Required => "required, which vens doesn't support yet",
    };
    format!(
        "scanner: {} ({})\naddress: {}\nports: command {}, control {}\ndevice status: {device_status}\npassword: {password}\n",
        text(reply.product_name().as_str()),
        text(reply.device_name().as_str()),
        Ipv4Addr::from(reply.address().octets()),
        reply.command_port().number(),
        reply.control_port().number(),
    )
}

/// Adds the paper path and the registered host to the identity.
#[must_use]
pub fn state(state: &State) -> String {
    let paper = match state.status.paper() {
        PaperPath::TopChute => "top chute",
        PaperPath::FrontSlot => "front slot",
        PaperPath::Absent => "none",
    };
    let registered = if state.status.registered_host().is_some() {
        "present"
    } else {
        "absent"
    };
    format!(
        "{}paper: {paper}\nregistered host: {registered}\n",
        found(&state.reply)
    )
}

fn sheets(count: u32) -> String {
    if count == 1 {
        "1 sheet".to_owned()
    } else {
        format!("{count} sheets")
    }
}

/// Lists the written images and the counts of a finished scan.
#[must_use]
pub fn scanned(scanned: &Scanned, written: &[PathBuf]) -> String {
    format!(
        "{}sheets: {}\nkeepalive replies: {}\n",
        paths(written),
        scanned.sheets,
        scanned.keepalive_replies
    )
}

/// Lists one path on each line.
#[must_use]
pub fn paths(written: &[PathBuf]) -> String {
    let mut lines = String::new();
    for path in written {
        lines.push_str(&path.display().to_string());
        lines.push('\n');
    }
    lines
}

/// Tells the user where the images of a failed scan stay.
#[must_use]
pub fn saved_before_failure(folder: &Path) -> String {
    format!(
        "the images above stay in {}; scan the remaining sheets again",
        folder.display()
    )
}

#[must_use]
pub const fn no_sheet() -> &'static str {
    "the scanner fed no sheet; check that the stack sits in the top chute"
}

fn greeting(error: &GreetingError) -> String {
    match error {
        GreetingError::Frame(_) => "the greeting is malformed".to_owned(),
        GreetingError::Refused { status } => {
            format!("the scanner refused the connection with status {status}")
        }
    }
}

fn header(error: &ReplyHeaderError) -> String {
    match error {
        ReplyHeaderError::Refused { status } => {
            format!("the scanner refused the command with frame status {status}")
        }
        ReplyHeaderError::Frame(_) | ReplyHeaderError::LengthBelowHeader { .. } => {
            "a reply header is malformed".to_owned()
        }
    }
}

fn channel<E: Display>(error: &ChannelError<E>) -> String {
    match error {
        ChannelError::Port(error) => error.to_string(),
        ChannelError::Greeting(error) => greeting(error),
        ChannelError::Header(error) => header(error),
        ChannelError::ReplyTooLarge { declared, capacity } => {
            format!("a reply of {declared} bytes exceeds the buffer of {capacity} bytes")
        }
    }
}

fn command<E: Display>(error: &ChannelError<E>) -> String {
    format!("the command connection failed: {}", channel(error))
}

fn refusal(refusal: ReserveRefusal) -> String {
    match refusal {
        ReserveRefusal::Occupied => "another computer holds the scanner; quit ScanSnap Home there, or close and open the scanner cover".to_owned(),
        ReserveRefusal::PasswordNecessary | ReserveRefusal::WrongPassword => {
            "the scanner asks for a password, which vens doesn't support yet".to_owned()
        }
        ReserveRefusal::UsbMode => "the scanner is in USB mode; unplug its USB cable".to_owned(),
        ReserveRefusal::HostIdNotRegistered | ReserveRefusal::HostIdNotValid => {
            "the scanner refused the registered host; pair it again with ScanSnap Home".to_owned()
        }
        ReserveRefusal::UnsupportedVersion => {
            "the scanner refused the protocol version of vens".to_owned()
        }
        ReserveRefusal::General | ReserveRefusal::SequenceError => {
            "the scanner refused the reserve; close and open its cover and try again".to_owned()
        }
        ReserveRefusal::Unrecognized(result) => {
            format!("the scanner refused the reserve with result {result}")
        }
    }
}

fn control<E: Display>(error: &ControlError<E>) -> String {
    match error {
        ControlError::Port(error) => format!("the control connection failed: {error}"),
        ControlError::Greeting(error) => {
            format!("the control connection failed: {}", greeting(error))
        }
        ControlError::Frame(_) => "a control reply is malformed".to_owned(),
        ControlError::Reserve(reason) => refusal(*reason),
        ControlError::Release(result) => {
            format!("the scanner refused the release with result {result}")
        }
    }
}

fn scsi(status: ScsiStatus) -> String {
    match status {
        ScsiStatus::Good => "good".to_owned(),
        ScsiStatus::CheckCondition => "check condition".to_owned(),
        ScsiStatus::Busy => "busy".to_owned(),
        ScsiStatus::Unrecognized(code) => format!("status {code}"),
    }
}

fn condition(condition: Condition) -> String {
    match condition {
        Condition::EndOfSide => "the scanner ended a side early".to_owned(),
        Condition::PaperJam => {
            "paper jam; open the scanner, remove the paper, and scan the remaining sheets again"
                .to_owned()
        }
        Condition::CoverOpen => "a scanner cover is open; close it and scan again".to_owned(),
        Condition::NoPaper => no_paper(),
        Condition::DoubleFeed => {
            "the scanner fed two sheets at once; separate the remaining sheets and scan them again"
                .to_owned()
        }
        Condition::NoDataYet => "the scanner had no image data ready".to_owned(),
        Condition::InvalidField => "the scanner refused a field of a command".to_owned(),
        Condition::Unrecognized {
            key,
            code,
            qualifier,
        } => format!("the scanner reported condition {key:X}/{code:02X}/{qualifier:02X}"),
    }
}

fn no_paper() -> String {
    "no paper in the top chute; load the stack and scan again".to_owned()
}

fn job(error: JobError) -> String {
    match error {
        JobError::NoPaper => no_paper(),
        JobError::UnsupportedPaperPath => {
            "paper in the front slot isn't supported yet; load it into the top chute".to_owned()
        }
        JobError::Refused { status } => format!(
            "the scanner refused a scan command ({})",
            scsi(ScsiStatus::Unrecognized(status))
        ),
        JobError::SenseUnavailable => "the scanner reported an error without details".to_owned(),
        JobError::Condition(reported) => condition(reported),
        JobError::Status(_) => "a status reply is malformed".to_owned(),
        JobError::Sense(_) => "an error report is malformed".to_owned(),
        JobError::Chunk(_) => "an image reply is malformed".to_owned(),
        JobError::PixelSize(_) => "a page size reply is malformed".to_owned(),
        JobError::HeightOutOfRange { lines } => {
            format!("a page of {lines} lines exceeds the height that an image can hold")
        }
        JobError::RetriesExhausted(Pause::Busy) => {
            "the scanner stayed busy; close and open its cover and scan again".to_owned()
        }
        JobError::RetriesExhausted(Pause::NoDataYet) => {
            "the scanner never delivered the image data".to_owned()
        }
        JobError::SequenceExhausted => {
            "a page needed more image chunks than the protocol can number".to_owned()
        }
    }
}

fn scan<E: Display, K: Display>(error: &ScanError<E, K>) -> String {
    match error {
        ScanError::Channel(error) => command(error),
        ScanError::Sink(error) => format!("an image couldn't be saved: {error}"),
        ScanError::Job(error) => job(*error),
    }
}

fn session<E: Display, K: Display>(error: &SessionError<E, K>) -> String {
    match error {
        SessionError::Command(error) => command(error),
        SessionError::StatusRefused(status) => {
            format!("the scanner refused the status request ({})", scsi(*status))
        }
        SessionError::Status(_) => "a status reply is malformed".to_owned(),
        SessionError::NotRegistered => {
            "the scanner has no registered host; pair it once with ScanSnap Home".to_owned()
        }
        SessionError::Reserve(error) => control(error),
        SessionError::Scan {
            failure,
            release: Ok(()),
        } => scan(failure),
        SessionError::Scan {
            failure,
            release: Err(error),
        } => format!(
            "{}; the release failed too: {}",
            scan(failure),
            control(error)
        ),
        SessionError::Release { summary, failure } => format!(
            "{} scanned, but {}",
            sheets(summary.sheets),
            control(failure)
        ),
    }
}

const fn connection(connection: Connection) -> &'static str {
    match connection {
        Connection::Command => "command",
        Connection::Control => "control",
    }
}

/// Explains a failure in the words that the user acts on.
#[must_use]
pub fn failure<E: Display, K: Display>(failure: &Failure<E, K>) -> String {
    match failure {
        Failure::Route { target, error } if target.is_broadcast() => format!(
            "this host has no route for a broadcast search ({error}); give the scanner's address with --address"
        ),
        Failure::Route { target, error } => format!("this host has no route to {target}: {error}"),
        Failure::Discovery(error) => format!("the discovery port failed: {error}"),
        Failure::NoReply { target, attempts } if target.is_broadcast() => format!(
            "no scanner answered {attempts} search probes; check that the scanner is on and on this network, give its address with --address, and let vens receive replies through the firewall"
        ),
        Failure::NoReply { target, attempts } => format!(
            "no scanner answered {attempts} search probes at {target}; check that the scanner is on and still has that address"
        ),
        Failure::Trigger(error) => format!("the port for button events couldn't open: {error}"),
        Failure::Clock(error) => {
            format!("the system clock can't give the reserve time ({error}); set the date and time")
        }
        Failure::Connect {
            connection: kind,
            error,
        } => format!(
            "the {} connection to the scanner failed: {error}",
            connection(*kind)
        ),
        Failure::Keepalive(error) => {
            format!("the keepalive port couldn't change its mode: {error}")
        }
        Failure::Command(error) => command(error),
        Failure::Control(error) => control(error),
        Failure::StatusRefused(status) => {
            format!("the scanner refused the status request ({})", scsi(*status))
        }
        Failure::Status(_) => "a status reply is malformed".to_owned(),
        Failure::Session(error) => session(error),
    }
}
