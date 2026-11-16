use std::sync::mpsc;

use vens_core::{
    Ipv4Address, MacAddress, Port,
    control::LocalTime,
    job::{Pause, Summary},
    jpeg::{self, JpegError},
    keepalive::Elapsed,
    scan::Side,
};
use vens_net::{
    channel::CommandChannel,
    control::ControlChannel,
    keepalive::Idle,
    record::Recorder,
    scan::{Clock, PageSink},
    session::{Host, SessionError, scan_once},
    trace::{Channel, Event},
};

use crate::{
    redact::{HOST_MAC, TRIGGER_PORT},
    replay::{Replay, ReplayError},
};

const REPLY_CAPACITY: usize = 0x0030_0040;

/// Records each pause and reads a clock that never moves.
#[derive(Debug, Default)]
pub struct Pauses(pub Vec<Pause>);

impl Clock for Pauses {
    fn wait(&mut self, pause: Pause) {
        self.0.push(pause);
    }

    fn now(&mut self) -> Elapsed {
        Elapsed::from_millis(0)
    }
}

/// Joins the chunks of each side and writes the reported height into its frame header, as the hardware command does.
#[derive(Debug, Default)]
pub struct Images {
    current: Vec<u8>,
    finished: Vec<(u32, Side, Vec<u8>)>,
}

impl PageSink for Images {
    type Error = JpegError;

    fn append(&mut self, _: u32, _: Side, data: &[u8]) -> Result<(), JpegError> {
        self.current.extend_from_slice(data);
        Ok(())
    }

    fn finish(&mut self, sheet: u32, side: Side, height: u16) -> Result<(), JpegError> {
        let mut image = std::mem::take(&mut self.current);
        jpeg::set_height(&mut image, height)?;
        self.finished.push((sheet, side, image));
        Ok(())
    }
}

pub type Outcome = Result<Summary, SessionError<ReplayError, JpegError>>;

#[derive(Debug)]
pub struct Replayed {
    pub outcome: Outcome,
    pub pauses: Vec<Pause>,
    pub images: Vec<(u32, Side, Vec<u8>)>,
    /// Tells whether the command and the control replays each consumed every recorded byte.
    pub finished: (bool, bool),
    /// Lists the command and control events in the order that the session produced them.
    pub order: Vec<Event>,
}

/// Replays the command and control channels of a fixture through one session of a host at `host_address`.
///
/// The host uses the documentation MAC, trigger port, and reserve time that redaction writes into a fixture.
#[must_use]
pub fn replay_session(events: &[Event], host_address: [u8; 4]) -> Option<Replayed> {
    let (log, order) = mpsc::channel();
    let command_port = Recorder::new(
        Replay::new(events, Channel::Command),
        Channel::Command,
        log.clone(),
    );
    let control_port = Recorder::new(Replay::new(events, Channel::Control), Channel::Control, log);
    let mac = MacAddress::new(HOST_MAC);
    let address = Ipv4Address::new(host_address);
    let mut command = CommandChannel::open(command_port, mac, address).ok()?;
    let mut control = ControlChannel::open(control_port).ok()?;
    let host = Host {
        mac,
        address,
        trigger_port: Port::new(TRIGGER_PORT),
        time: LocalTime::new(2026, 1, 1, 0, 0, 0)?,
    };
    let mut pauses = Pauses::default();
    let mut images = Images::default();
    let mut buffer = vec![0_u8; REPLY_CAPACITY];
    let outcome = scan_once(
        &mut command,
        &mut control,
        &mut Idle,
        host,
        &mut pauses,
        &mut images,
        &mut buffer,
    );
    Some(Replayed {
        outcome,
        pauses: pauses.0,
        images: images.finished,
        finished: (
            command.into_stream().into_port().finished(),
            control.into_stream().into_port().finished(),
        ),
        order: order.try_iter().collect(),
    })
}
