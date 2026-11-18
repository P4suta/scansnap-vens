use std::{
    fmt::Debug,
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread::{self, Scope},
    time::{Duration, Instant},
};

use vens_core::jpeg::{components, frame_size};
use vens_escl::{
    bridge::{Bridge, Effect, Input, Moment, Refusal, Request, Response, Side},
    uuid::Uuid,
};
use vens_net::{host::SystemNetwork, network::Network};

use crate::{
    button,
    http::{Limits, read_request, refusal, refusal_status, status, write_refusal, write_response},
    job::{JobEvent, Report, run_job},
    request::escl_request,
};

const CONNECTIONS_PER_LISTENER: usize = 32;
const PEER_CHECK: Duration = Duration::from_millis(10);
const PEER_POLL: Duration = Duration::from_millis(100);
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

pub struct Capture {
    pub folder: PathBuf,
    pub network: SystemNetwork,
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
    reply: Sender<Reply>,
    starter: Option<TcpStream>,
}

enum Acknowledge {
    None,
    Created(Sender<bool>),
    Page { id: Uuid, events: Sender<Event> },
}

struct Reply {
    response: Response<Vec<u8>>,
    acknowledge: Acknowledge,
}

impl Reply {
    fn written(&mut self, success: bool) {
        match std::mem::replace(&mut self.acknowledge, Acknowledge::None) {
            Acknowledge::Created(decision) => {
                let _ = decision.send(success);
            }
            Acknowledge::Page { id, events } => {
                let returned = if success {
                    None
                } else if let Response::Page(page) =
                    std::mem::replace(&mut self.response, Response::NotFound)
                {
                    Some(page)
                } else {
                    None
                };
                let _ = events.send(Event::Transfer { id, returned });
            }
            Acknowledge::None => {}
        }
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        self.written(false);
    }
}

enum Event {
    Request {
        request: Request,
        responder: Responder,
    },
    Job(JobEvent),
    Creation(Sender<bool>),
    Transfer {
        id: Uuid,
        returned: Option<Vec<u8>>,
    },
    Line(String),
}

struct ToOwner {
    events: Sender<Event>,
    peer: Option<TcpStream>,
    timeout: Duration,
}

impl Report for ToOwner {
    fn report(&mut self, event: JobEvent) {
        let _ = self.events.send(Event::Job(event));
    }

    fn client_waiting(&self) -> bool {
        self.peer.as_ref().is_none_or(peer_waiting)
    }

    fn created(&mut self) -> bool {
        let (decision, observed) = mpsc::channel();
        let delivered = self.events.send(Event::Creation(decision)).is_ok()
            && observed.recv_timeout(self.timeout) == Ok(true);
        self.peer = None;
        delivered
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

impl<'a> Deadline<'a> {
    fn new(stream: &'a mut TcpStream, limit: Duration) -> Self {
        Self {
            stream,
            started: Instant::now(),
            limit,
        }
    }

    fn remaining(&self) -> io::Result<Duration> {
        let remaining = self.limit.saturating_sub(self.started.elapsed());
        if remaining.is_zero() {
            return Err(io::Error::from(io::ErrorKind::TimedOut));
        }
        Ok(remaining)
    }
}

impl Read for Deadline<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.stream.set_read_timeout(Some(self.remaining()?))?;
        self.stream.read(buffer)
    }
}

impl Write for Deadline<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

fn peer_waiting(stream: &TcpStream) -> bool {
    match stream.peek(&mut [0_u8; 1]) {
        Ok(read) => read != 0,
        Err(error) => matches!(
            error.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
        ),
    }
}

fn await_reply(answers: &Receiver<Reply>, stream: &TcpStream, timeout: Duration) -> Option<Reply> {
    let started = Instant::now();
    loop {
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return None;
        }
        match answers.recv_timeout(remaining.min(PEER_POLL)) {
            Ok(reply) => return Some(reply),
            Err(RecvTimeoutError::Timeout) if peer_waiting(stream) => {}
            Err(RecvTimeoutError::Disconnected | RecvTimeoutError::Timeout) => return None,
        }
    }
}

fn answer(mut stream: TcpStream, peer: SocketAddr, config: Config, events: &Sender<Event>) {
    let request = match read_request(
        &mut Deadline::new(&mut stream, config.request_timeout),
        config.limits,
    ) {
        Ok(request) => request,
        Err(error) => {
            let text = match refusal(error) {
                Some(refusal) => {
                    let (code, _) = refusal_status(refusal);
                    let written = write_refusal(
                        &mut Deadline::new(&mut stream, config.write_timeout),
                        refusal,
                    )
                    .err()
                    .map_or_else(String::new, |failure| format!(", write failed: {failure}"));
                    format!("{peer} unreadable request {error:?} {code}{written}")
                }
                None => format!("{peer} request ended: {error:?}"),
            };
            return line(events, text);
        }
    };
    if let Err(error) = stream.set_read_timeout(Some(PEER_CHECK)) {
        return line(events, format!("{peer} timeouts failed: {error}"));
    }
    let (reply, answers) = mpsc::channel();
    let escl = escl_request(&request);
    let starter = if matches!(escl, Request::CreateJob(Ok(_))) {
        match stream.try_clone() {
            Ok(pending) => Some(pending),
            Err(error) => {
                return line(
                    events,
                    format!("{peer} couldn't track the job request: {error}"),
                );
            }
        }
    } else {
        None
    };
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
        starter,
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
    let Some(mut reply) = await_reply(&answers, &stream, config.write_timeout) else {
        return;
    };
    let written = if matches!(reply.acknowledge, Acknowledge::None) || peer_waiting(&stream) {
        write_response(
            &mut Deadline::new(&mut stream, config.write_timeout),
            &reply.response,
        )
    } else {
        Err(io::Error::from(io::ErrorKind::BrokenPipe))
    };
    if let Err(error) = &written {
        line(events, format!("{peer} write failed: {error}"));
    }
    reply.written(written.is_ok());
}

struct ConnectionSlot(Sender<()>);

impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

fn accept<'scope>(
    scope: &'scope Scope<'scope, '_>,
    listener: &TcpListener,
    config: Config,
    events: &Sender<Event>,
) {
    let (completion, completed) = mpsc::channel();
    let mut active = 0_usize;
    loop {
        match listener.accept() {
            Ok((stream, peer)) => {
                active = active.saturating_sub(completed.try_iter().count());
                if active >= CONNECTIONS_PER_LISTENER {
                    continue;
                }
                let messages = events.clone();
                let completion = completion.clone();
                let spawned = thread::Builder::new()
                    .name("vensd request".to_owned())
                    .spawn_scoped(scope, move || {
                        let _slot = ConnectionSlot(completion);
                        answer(stream, peer, config, &messages);
                    });
                match spawned {
                    Ok(_) => active = active.saturating_add(1),
                    Err(error) => line(events, format!("a request thread couldn't start: {error}")),
                }
            }
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
    mut report: ToOwner,
) {
    let Some(mut network) = networks.network() else {
        return report.report(JobEvent::Refused {
            refusal: Refusal::Unreachable,
            detail: "no network for the job".to_owned(),
        });
    };
    let events = report.events.clone();
    let spawned = thread::Builder::new()
        .name("vensd job".to_owned())
        .spawn_scoped(scope, move || run_job(&mut network, target, &mut report));
    if let Err(error) = spawned {
        let _ = events.send(Event::Job(JobEvent::Refused {
            refusal: Refusal::Unreachable,
            detail: format!("the job thread couldn't start: {error}"),
        }));
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

type OwnerInput = Input<Responder, Vec<u8>>;

fn input<L: Log>(event: Event, log: &mut L) -> Option<(OwnerInput, Option<Sender<bool>>)> {
    let input = match event {
        Event::Request { request, responder } => Input::Request { request, responder },
        Event::Creation(decision) => {
            log.record("job reserved the scanner");
            return Some((Input::Reserved, Some(decision)));
        }
        Event::Job(JobEvent::Reserved) => {
            log.record("job reserved the scanner");
            Input::Reserved
        }
        Event::Job(JobEvent::Abandoned { detail }) => {
            let detail = detail.map_or_else(String::new, |detail| format!(" ({detail})"));
            log.record(&format!("job client left before the scan{detail}"));
            Input::Abandoned
        }
        Event::Job(JobEvent::Refused { refusal, detail }) => {
            log.record(&format!("job refused: {refusal:?} ({detail})"));
            Input::Refused(refusal)
        }
        Event::Job(JobEvent::Page { side, page }) => {
            log.record(&page_line(side, &page));
            Input::Page { side, page }
        }
        Event::Transfer { id, returned } => Input::PageWritten { id, returned },
        Event::Job(JobEvent::Finished { ending, detail }) => {
            let detail = detail.map_or_else(String::new, |detail| format!(" ({detail})"));
            log.record(&format!("job finished: {ending:?}{detail}"));
            Input::Finished(ending)
        }
        Event::Line(text) => {
            log.record(&text);
            return None;
        }
    };
    Some((input, None))
}

fn respond<L: Log>(responder: &Responder, reply: Reply, log: &mut L) {
    let (code, _) = status(&reply.response);
    let client = responder
        .job
        .as_ref()
        .map_or_else(String::new, |job| format!(" {}", job.client));
    log.record(&format!(
        "{} {code}{}{client}",
        responder.line,
        reason(&reply.response)
    ));
    if let (400..=599, Some(job)) = (code, &responder.job) {
        log.record(&format!("{} body {:?}", responder.line, job.body));
    }
    let _ = responder.reply.send(reply);
}

fn own<'scope, S: Networks, L: Log>(
    scope: &'scope Scope<'scope, '_>,
    inbox: &Receiver<Event>,
    events: &Sender<Event>,
    networks: &mut S,
    worker: Option<&Sender<ToOwner>>,
    config: Config,
    log: &mut L,
) {
    let started = Instant::now();
    let mut bridge = Bridge::<Responder, Vec<u8>, QUEUE>::new(config.uuid, config.seed);
    while let Ok(event) = inbox.recv() {
        let Some((mut input, mut created)) = input(event, log) else {
            continue;
        };
        let mut peer = if let Input::Request { responder, .. } = &mut input {
            responder.starter.take()
        } else {
            None
        };
        let (next, effects) = bridge.handle(input, now(started));
        bridge = next;
        for effect in effects {
            match effect {
                Effect::Reply {
                    responder,
                    response,
                } => {
                    let acknowledge = created
                        .take()
                        .map_or(Acknowledge::None, Acknowledge::Created);
                    respond(
                        &responder,
                        Reply {
                            response,
                            acknowledge,
                        },
                        log,
                    );
                }
                Effect::Transfer {
                    responder,
                    id,
                    page,
                } => {
                    respond(
                        &responder,
                        Reply {
                            response: Response::Page(page),
                            acknowledge: Acknowledge::Page {
                                id,
                                events: events.clone(),
                            },
                        },
                        log,
                    );
                }
                Effect::Start(_) => {
                    let Some(peer) = peer.take() else {
                        let _ = events.send(Event::Job(JobEvent::Abandoned { detail: None }));
                        continue;
                    };
                    let report = ToOwner {
                        events: events.clone(),
                        peer: Some(peer),
                        timeout: config.write_timeout,
                    };
                    if let Some(worker) = worker {
                        if let Err(mut dropped) = worker.send(report) {
                            dropped.0.report(JobEvent::Refused {
                                refusal: Refusal::Unreachable,
                                detail: "the button worker stopped".to_owned(),
                            });
                        }
                    } else {
                        start_job(scope, networks, config.target, report);
                    }
                }
            }
        }
    }
}

/// Serves eSCL clients on the listener and runs each job on a network from `networks`.
///
/// One thread owns the bridge state, and the others only send it events: each accepted request, and each step of the running job.
pub fn serve<S: Networks, L: Log>(listeners: &[TcpListener], networks: S, config: Config, log: L) {
    serve_configured(listeners, networks, config, log, None);
}

/// Serves eSCL and optionally starts a single worker for HTTP jobs and the Scan button.
pub fn serve_configured<S: Networks, L: Log>(
    listeners: &[TcpListener],
    mut networks: S,
    config: Config,
    mut log: L,
    capture: Option<Capture>,
) {
    let (events, inbox) = mpsc::channel();
    thread::scope(|scope| {
        let worker = if let Some(capture) = capture {
            let (jobs, work) = mpsc::channel();
            let messages = events.clone();
            let started = thread::Builder::new()
                .name("vensd scanner".to_owned())
                .spawn_scoped(scope, move || {
                    button::run(
                        capture.network,
                        config.target,
                        &capture.folder,
                        &work,
                        |text| {
                            line(&messages, text.to_owned());
                        },
                    );
                });
            if let Err(error) = started {
                log.record(&format!("the scanner worker couldn't start: {error}"));
                return;
            }
            Some(jobs)
        } else {
            None
        };
        for listener in listeners {
            let acceptor_events = events.clone();
            let spawned = thread::Builder::new()
                .name("vensd acceptor".to_owned())
                .spawn_scoped(scope, move || {
                    accept(scope, listener, config, &acceptor_events);
                });
            if let Err(error) = spawned {
                line(&events, format!("an acceptor couldn't start: {error}"));
            }
        }
        own(
            scope,
            &inbox,
            &events,
            &mut networks,
            worker.as_ref(),
            config,
            &mut log,
        );
    });
}
