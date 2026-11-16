use std::{
    fmt::Debug,
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, Scope},
    time::{Duration, Instant},
};

use vens_escl::{
    bridge::{Bridge, Effect, Input, Moment, Refusal, Request, Response},
    uuid::Uuid,
};
use vens_net::network::Network;

use crate::{
    http::{Limits, read_request, refusal, refusal_status, status, write_refusal, write_response},
    job::{JobEvent, Report, run_job},
    request::escl_request,
};

/// The connections that the daemon reads at the same time, so a waiting request leaves room for others.
const ACCEPTORS: usize = 4;
/// The pages that a job keeps for its client, both sides of 128 sheets.
const QUEUE: usize = 256;
/// The pause after a failed accept, so a lasting failure doesn't spin.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub uuid: Uuid,
    pub seed: [u8; 16],
    /// The scanner's address, or the broadcast address to search for it.
    pub target: Ipv4Addr,
    pub limits: Limits,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
}

/// Supplies the network that each job uses.
pub trait Networks {
    type Network: Network<Error: Debug> + Send + 'static;

    fn network(&mut self) -> Option<Self::Network>;
}

/// Keeps the daemon's log lines.
pub trait Log {
    fn record(&mut self, line: &str);
}

struct Responder {
    line: String,
    reply: Sender<Response<Vec<u8>>>,
}

enum Event {
    Request {
        request: Request,
        responder: Responder,
    },
    Job(JobEvent),
    Line(String),
}

struct ToOwner(Sender<Event>);

impl Report for ToOwner {
    fn report(&mut self, event: JobEvent) {
        let _ = self.0.send(Event::Job(event));
    }
}

fn line(events: &Sender<Event>, text: String) {
    let _ = events.send(Event::Line(text));
}

fn answer(mut stream: TcpStream, peer: SocketAddr, config: Config, events: &Sender<Event>) {
    let timeouts = stream
        .set_read_timeout(Some(config.read_timeout))
        .and_then(|()| stream.set_write_timeout(Some(config.write_timeout)));
    if let Err(error) = timeouts {
        return line(events, format!("{peer} timeouts failed: {error}"));
    }
    let request = match read_request(&mut stream, config.limits) {
        Ok(request) => request,
        Err(error) => {
            let text = match refusal(error) {
                Some(refusal) => {
                    let (code, _) = refusal_status(refusal);
                    let written = write_refusal(&mut stream, refusal)
                        .err()
                        .map_or_else(String::new, |failure| format!(", write failed: {failure}"));
                    format!("{peer} unreadable request {error:?} {code}{written}")
                }
                None => format!("{peer} request ended: {error:?}"),
            };
            return line(events, text);
        }
    };
    let (reply, answers) = mpsc::channel();
    let responder = Responder {
        line: format!("{peer} {} {}", request.method, request.target),
        reply,
    };
    let request = escl_request(&request);
    if events.send(Event::Request { request, responder }).is_err() {
        return;
    }
    let Ok(response) = answers.recv() else {
        return;
    };
    if let Err(error) = write_response(&mut stream, &response) {
        line(events, format!("{peer} write failed: {error}"));
    }
}

fn accept(listener: &TcpListener, config: Config, events: &Sender<Event>) {
    loop {
        match listener.accept() {
            Ok((stream, peer)) => answer(stream, peer, config, events),
            Err(error) => {
                line(events, format!("accept failed: {error}"));
                thread::sleep(ACCEPT_BACKOFF);
            }
        }
    }
}

fn now(started: Instant) -> Moment {
    Moment::from_millis(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX))
}

fn start_job<'scope, S: Networks>(
    scope: &'scope Scope<'scope, '_>,
    networks: &mut S,
    target: Ipv4Addr,
    events: &Sender<Event>,
) {
    let mut report = ToOwner(events.clone());
    match networks.network() {
        Some(mut network) => {
            scope.spawn(move || run_job(&mut network, target, &mut report));
        }
        None => report.report(JobEvent::Refused {
            refusal: Refusal::Unreachable,
            detail: "no network for the job".to_owned(),
        }),
    }
}

fn input<L: Log>(event: Event, log: &mut L) -> Option<Input<Responder, Vec<u8>>> {
    match event {
        Event::Request { request, responder } => Some(Input::Request { request, responder }),
        Event::Job(JobEvent::Reserved) => {
            log.record("job reserved the scanner");
            Some(Input::Reserved)
        }
        Event::Job(JobEvent::Refused { refusal, detail }) => {
            log.record(&format!("job refused: {refusal:?} ({detail})"));
            Some(Input::Refused(refusal))
        }
        Event::Job(JobEvent::Page { side, page }) => Some(Input::Page { side, page }),
        Event::Job(JobEvent::Finished { ending, detail }) => {
            let detail = detail.map_or_else(String::new, |detail| format!(" ({detail})"));
            log.record(&format!("job finished: {ending:?}{detail}"));
            Some(Input::Finished(ending))
        }
        Event::Line(text) => {
            log.record(&text);
            None
        }
    }
}

fn own<'scope, S: Networks, L: Log>(
    scope: &'scope Scope<'scope, '_>,
    inbox: &Receiver<Event>,
    events: &Sender<Event>,
    networks: &mut S,
    config: Config,
    log: &mut L,
) {
    let started = Instant::now();
    let mut bridge = Bridge::<Responder, Vec<u8>, QUEUE>::new(config.uuid, config.seed);
    while let Ok(event) = inbox.recv() {
        let Some(input) = input(event, log) else {
            continue;
        };
        let (next, effects) = bridge.handle(input, now(started));
        bridge = next;
        for effect in effects {
            match effect {
                Effect::Reply {
                    responder,
                    response,
                } => {
                    let (code, _) = status(&response);
                    log.record(&format!("{} {code}", responder.line));
                    let _ = responder.reply.send(response);
                }
                Effect::Start(_) => start_job(scope, networks, config.target, events),
            }
        }
    }
}

/// Serves eSCL clients on the listener and runs each job on a network from `networks`.
///
/// One thread owns the bridge state, and the others only send it events: each accepted request, and each step of the running job.
pub fn serve<S: Networks, L: Log>(
    listener: &TcpListener,
    mut networks: S,
    config: Config,
    mut log: L,
) {
    let (events, inbox) = mpsc::channel();
    thread::scope(|scope| {
        for _ in 0..ACCEPTORS {
            let events = events.clone();
            scope.spawn(move || accept(listener, config, &events));
        }
        own(scope, &inbox, &events, &mut networks, config, &mut log);
    });
}
