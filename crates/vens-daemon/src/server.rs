use std::{
    fmt::Debug,
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, Scope},
    time::{Duration, Instant},
};

use vens_core::jpeg::{components, frame_size};
use vens_escl::{
    bridge::{Bridge, Effect, Input, Moment, Refusal, Request, Response, Side},
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
/// The characters of a job request body that the log keeps.
const BODY_EXCERPT: usize = 2048;

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub uuid: Uuid,
    pub seed: [u8; 16],
    /// The scanner's address, or the broadcast address to search for it.
    pub target: Ipv4Addr,
    pub limits: Limits,
    /// The time that a whole request may take to arrive.
    pub request_timeout: Duration,
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

/// The client, media type, and body of a job request, which the log keeps beside the answer.
struct JobRequest {
    client: String,
    body: String,
}

struct Responder {
    line: String,
    job: Option<JobRequest>,
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

/// A connection whose reads share one deadline, so a client can't stretch a request by trickling its bytes.
struct Deadline<'a> {
    stream: &'a mut TcpStream,
    started: Instant,
    limit: Duration,
}

impl Read for Deadline<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = self.limit.saturating_sub(self.started.elapsed());
        if remaining.is_zero() {
            return Err(io::Error::from(io::ErrorKind::TimedOut));
        }
        self.stream.set_read_timeout(Some(remaining))?;
        self.stream.read(buffer)
    }
}

impl Write for Deadline<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stream.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

fn answer(mut stream: TcpStream, peer: SocketAddr, config: Config, events: &Sender<Event>) {
    if let Err(error) = stream.set_write_timeout(Some(config.write_timeout)) {
        return line(events, format!("{peer} timeouts failed: {error}"));
    }
    let mut deadline = Deadline {
        stream: &mut stream,
        started: Instant::now(),
        limit: config.request_timeout,
    };
    let request = match read_request(&mut deadline, config.limits) {
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
    let escl = escl_request(&request);
    let job = matches!(escl, Request::CreateJob(_) | Request::Malformed).then(|| JobRequest {
        client: format!(
            "client {:?}, type {:?}",
            request.user_agent.as_deref().unwrap_or("unnamed"),
            request.content_type.as_deref().unwrap_or("none")
        ),
        body: String::from_utf8_lossy(&request.body)
            .chars()
            .take(BODY_EXCERPT)
            .collect(),
    });
    let responder = Responder {
        line: format!(
            "{peer} {} {}",
            request.method.escape_debug(),
            request.target.escape_debug()
        ),
        job,
        reply,
    };
    if events
        .send(Event::Request {
            request: escl,
            responder,
        })
        .is_err()
    {
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
    let mut fallback = ToOwner(events.clone());
    let Some(mut network) = networks.network() else {
        return fallback.report(JobEvent::Refused {
            refusal: Refusal::Unreachable,
            detail: "no network for the job".to_owned(),
        });
    };
    let mut report = ToOwner(events.clone());
    let spawned = thread::Builder::new()
        .name("vensd job".to_owned())
        .spawn_scoped(scope, move || run_job(&mut network, target, &mut report));
    if let Err(error) = spawned {
        fallback.report(JobEvent::Refused {
            refusal: Refusal::Unreachable,
            detail: format!("the job thread couldn't start: {error}"),
        });
    }
}

fn page_line(side: Side, page: &[u8]) -> String {
    match (frame_size(page), components(page)) {
        (Ok(frame), Ok(count)) => format!(
            "page {side:?}: {}x{}, components {count}",
            frame.width, frame.height
        ),
        (Err(error), _) | (Ok(_), Err(error)) => {
            format!("page {side:?}: unreadable frame header {error:?}")
        }
    }
}

fn reason<P>(response: &Response<P>) -> String {
    match response {
        Response::Conflict(conflict) => format!(" {conflict:?}"),
        Response::Unavailable(unavailable) => format!(" {unavailable:?}"),
        Response::Capabilities(_)
        | Response::Status(_)
        | Response::Created(_)
        | Response::Page(_)
        | Response::Deleted
        | Response::NotFound
        | Response::MethodNotAllowed(_)
        | Response::BadRequest => String::new(),
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
        Event::Job(JobEvent::Page { side, page }) => {
            log.record(&page_line(side, &page));
            Some(Input::Page { side, page })
        }
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
                    let client = responder
                        .job
                        .as_ref()
                        .map_or_else(String::new, |job| format!(" {}", job.client));
                    log.record(&format!(
                        "{} {code}{}{client}",
                        responder.line,
                        reason(&response)
                    ));
                    if let (400..=599, Some(job)) = (code, &responder.job) {
                        log.record(&format!("{} body {:?}", responder.line, job.body));
                    }
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
    listeners: &[TcpListener],
    mut networks: S,
    config: Config,
    mut log: L,
) {
    let (events, inbox) = mpsc::channel();
    thread::scope(|scope| {
        for listener in listeners {
            for _ in 0..ACCEPTORS {
                let acceptor_events = events.clone();
                let spawned = thread::Builder::new()
                    .name("vensd acceptor".to_owned())
                    .spawn_scoped(scope, move || accept(listener, config, &acceptor_events));
                if let Err(error) = spawned {
                    line(&events, format!("an acceptor couldn't start: {error}"));
                }
            }
        }
        own(scope, &inbox, &events, &mut networks, config, &mut log);
    });
}
