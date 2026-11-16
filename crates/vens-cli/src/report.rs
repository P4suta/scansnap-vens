use std::{fmt::Display, path::PathBuf};

use vens_core::{
    command::ScsiStatus,
    control::{GreetingError, ReserveRefusal},
    discovery::{Availability, Password, SearchReply},
    job::{JobError, Pause},
    sense::Condition,
    status::PaperPath,
};
use vens_net::{
    channel::ChannelError, control::ControlError, scan::ScanError, search::SearchError,
    session::SessionError,
};

use crate::{
    invocation::UsageError,
    operation::{Failure, Scanned, State},
};

const USAGE: &str = "usage: vens <subcommand> [options]

  vens discover [--address <ipv4>]
      find the scanner and show its identity
  vens status [--address <ipv4>]
      show the identity, the paper path, and the registered host
  vens scan <folder> [--address <ipv4>] [--trace <file>]
      scan the stack in the top chute into the folder, one image for each side
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
    let address = reply.address().octets();
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
        "scanner: {} ({})\naddress: {}.{}.{}.{}\nports: command {}, control {}\ndevice status: {device_status}\npassword: {password}\n",
        text(reply.product_name().as_str()),
        text(reply.device_name().as_str()),
        address[0],
        address[1],
        address[2],
        address[3],
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

/// Lists the written images and the counts of a finished scan.
#[must_use]
pub fn scanned(scanned: &Scanned, written: &[PathBuf]) -> String {
    let mut files = String::new();
    for path in written {
        files.push_str(&path.display().to_string());
        files.push('\n');
    }
    format!(
        "{files}sheets: {}\nkeepalive replies: {}\n",
        scanned.sheets, scanned.keepalive_replies
    )
}

fn greeting(error: &GreetingError) -> String {
    match error {
        GreetingError::Frame(frame) => format!("the greeting is malformed ({frame:?})"),
        GreetingError::Refused { status } => {
            format!("the scanner refused the connection with status {status}")
        }
    }
}

fn channel<E: Display>(error: &ChannelError<E>) -> String {
    match error {
        ChannelError::Port(error) => error.to_string(),
        ChannelError::Greeting(error) => greeting(error),
        ChannelError::Header(header) => format!("a reply header is malformed ({header:?})"),
        ChannelError::ReplyTooLarge { declared, capacity } => {
            format!("a reply of {declared} bytes exceeds the buffer of {capacity} bytes")
        }
    }
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
        ControlError::Port(error) => error.to_string(),
        ControlError::Greeting(error) => greeting(error),
        ControlError::Frame(frame) => format!("a control reply is malformed ({frame:?})"),
        ControlError::Reserve(reason) => refusal(*reason),
        ControlError::Release(result) => {
            format!("the scanner refused the release with result {result}")
        }
    }
}

fn condition(condition: Condition) -> String {
    match condition {
        Condition::EndOfSide => "the scanner ended a side early".to_owned(),
        Condition::PaperJam => {
            "paper jam; open the scanner, remove the paper, and scan again".to_owned()
        }
        Condition::CoverOpen => "a scanner cover is open".to_owned(),
        Condition::NoPaper => "no paper in the top chute".to_owned(),
        Condition::DoubleFeed => "the scanner fed two sheets at once".to_owned(),
        Condition::NoDataYet => "the scanner had no image data ready".to_owned(),
        Condition::InvalidField => "the scanner refused a field of a command".to_owned(),
        Condition::Unrecognized {
            key,
            code,
            qualifier,
        } => format!("the scanner reported condition {key:X}/{code:02X}/{qualifier:02X}"),
    }
}

fn job(error: JobError) -> String {
    match error {
        JobError::NoPaper => "no paper in the top chute".to_owned(),
        JobError::UnsupportedPaperPath => {
            "paper in the front slot isn't supported yet; load it into the top chute".to_owned()
        }
        JobError::Refused { status } => {
            format!("the scanner refused a scan command with status {status}")
        }
        JobError::SenseUnavailable => "the scanner reported an error without details".to_owned(),
        JobError::Condition(reported) => condition(reported),
        JobError::Status(status) => format!("a status reply is malformed ({status:?})"),
        JobError::Sense(sense) => format!("an error report is malformed ({sense:?})"),
        JobError::Chunk(chunk) => format!("an image reply is malformed ({chunk:?})"),
        JobError::PixelSize(size) => format!("a page size reply is malformed ({size:?})"),
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
        ScanError::Channel(error) => format!("the command connection failed: {}", channel(error)),
        ScanError::Sink(error) => format!("an image couldn't be saved: {error}"),
        ScanError::Job(error) => job(*error),
    }
}

fn status_refused(status: ScsiStatus) -> String {
    format!("the scanner refused the status request ({status:?})")
}

fn session<E: Display, K: Display>(error: &SessionError<E, K>) -> String {
    match error {
        SessionError::Command(error) => {
            format!("the command connection failed: {}", channel(error))
        }
        SessionError::StatusRefused(status) => status_refused(*status),
        SessionError::Status(status) => format!("a status reply is malformed ({status:?})"),
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
            "{} sheets were scanned, but {}",
            summary.sheets,
            control(failure)
        ),
    }
}

/// Explains a failure in the words that the user acts on.
#[must_use]
pub fn failure<E: Display, K: Display>(failure: &Failure<E, K>) -> String {
    match failure {
        Failure::Network(error) => format!("the network failed: {error}"),
        Failure::Search(SearchError::Port(error)) => format!("the discovery port failed: {error}"),
        Failure::Search(SearchError::NoReply { attempts }) => format!(
            "no scanner answered {attempts} search probes; give its address with --address, and let vens receive replies through the firewall"
        ),
        Failure::Command(error) => format!("the command connection failed: {}", channel(error)),
        Failure::Control(error) => format!("the control connection failed: {}", control(error)),
        Failure::StatusRefused(status) => status_refused(*status),
        Failure::Status(status) => format!("a status reply is malformed ({status:?})"),
        Failure::Session(error) => session(error),
    }
}
