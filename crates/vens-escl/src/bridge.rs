use crate::{
    document::{AdfState, Capabilities, JobInfo, JobState, ScannerState, ScannerStatus},
    route::{JobPath, Method, Unrouted},
    settings::{JobSettings, Sides, Unsupported},
    uuid::Uuid,
};

/// How long the feeder state that a refused job request found stays in the status.
pub const HINT_MILLIS: u64 = 10_000;
/// How long a finished job stays after its end or its last page request, whichever came later.
pub const RETENTION_MILLIS: u64 = 60_000;

/// A reading of the owner's monotonic clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Moment(u64);

impl Moment {
    #[must_use]
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    const fn after(self, millis: u64) -> Self {
        Self(self.0.saturating_add(millis))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Front,
    Back,
}

/// Why the job thread gave up before the reserve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NoPaper,
    Busy,
    Unreachable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    Jam,
    /// The scanner detected two sheets fed at once.
    DoubleFeed,
    Scanner,
    /// The page queue was full, so the bridge dropped a page.
    Lost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    Completed,
    Failed(Fault),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Capabilities,
    Status,
    CreateJob(Result<JobSettings, Unsupported>),
    NextDocument(Uuid),
    DeleteJob(Uuid),
    Unrouted(Unrouted),
    Malformed,
}

/// A client request with the handle that answers it, or an event of the job thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input<R, P> {
    Request { request: Request, responder: R },
    Reserved,
    Refused(Refusal),
    Page { side: Side, page: P },
    Finished(Ending),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conflict {
    Unsupported(Unsupported),
    NoPaper,
    Failed(Fault),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    JobInProgress,
    ScannerBusy,
    ScannerUnreachable,
    /// A newer request for the next page replaced this one.
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response<P> {
    Capabilities(Capabilities),
    Status(ScannerStatus),
    Created(JobPath),
    Page(P),
    Deleted,
    NotFound,
    /// The answer to another method names the method that the resource allows.
    MethodNotAllowed(Method),
    BadRequest,
    Conflict(Conflict),
    Unavailable(Unavailable),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect<R, P> {
    Reply {
        responder: R,
        response: Response<P>,
    },
    /// Starts the job thread, which reports the reserve, each page, and the end.
    Start(JobSettings),
}

/// The effects of one input, at most two.
#[derive(Debug)]
pub struct Effects<R, P>([Option<Effect<R, P>>; 2]);

impl<R, P> Effects<R, P> {
    const fn none() -> Self {
        Self([None, None])
    }

    const fn one(effect: Effect<R, P>) -> Self {
        Self([Some(effect), None])
    }

    const fn reply(responder: R, response: Response<P>) -> Self {
        Self::one(Effect::Reply {
            responder,
            response,
        })
    }
}

impl<R, P> IntoIterator for Effects<R, P> {
    type Item = Effect<R, P>;
    type IntoIter = core::iter::Flatten<core::array::IntoIter<Option<Effect<R, P>>, 2>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter().flatten()
    }
}

/// A first-in, first-out ring of at most `N` pages.
#[derive(Debug, Clone)]
struct Queue<P, const N: usize> {
    slots: [Option<P>; N],
    head: usize,
    len: usize,
}

impl<P, const N: usize> Queue<P, N> {
    fn new() -> Self {
        Self {
            slots: core::array::from_fn(|_| None),
            head: 0,
            len: 0,
        }
    }

    fn push(&mut self, page: P) -> Result<(), P> {
        if self.len >= N {
            return Err(page);
        }
        let slot = self
            .head
            .checked_add(self.len)
            .and_then(|end| end.checked_rem(N))
            .and_then(|index| self.slots.get_mut(index));
        match slot {
            Some(slot) => {
                *slot = Some(page);
                self.len = self.len.saturating_add(1);
                Ok(())
            }
            None => Err(page),
        }
    }

    fn pop(&mut self) -> Option<P> {
        let next = self.head.checked_add(1)?.checked_rem(N)?;
        let page = self.slots.get_mut(self.head)?.take()?;
        self.head = next;
        self.len = self.len.saturating_sub(1);
        Some(page)
    }

    const fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn count(&self) -> u32 {
        u32::try_from(self.len).unwrap_or(u32::MAX)
    }
}

#[derive(Debug, Clone)]
enum Phase<R> {
    /// The job request waits for the reserve.
    Starting(R),
    /// The scanner feeds sheets, and a request for the next page may wait while the queue is empty.
    Scanning {
        waiter: Option<R>,
    },
    /// The client deleted the job, and the scanner still feeds its stack.
    Canceled,
    Finished(Ending),
}

#[derive(Debug, Clone)]
struct Job<R, P, const N: usize> {
    id: Uuid,
    sides: Sides,
    phase: Phase<R>,
    pages: Queue<P, N>,
    delivered: u32,
    lost: u32,
    touched: Moment,
}

#[derive(Debug, Clone, Copy)]
struct Hint {
    adf: AdfState,
    until: Moment,
}

/// The state of a bridge that runs one job at a time and keeps up to `N` pages for its client.
#[derive(Debug, Clone)]
pub struct Bridge<R, P, const N: usize> {
    uuid: Uuid,
    seed: [u8; 16],
    jobs: u32,
    job: Option<Job<R, P, N>>,
    hint: Option<Hint>,
}

fn job_id(seed: [u8; 16], number: u32) -> Uuid {
    let mut bytes = seed;
    for (slot, byte) in bytes.iter_mut().skip(12).zip(number.to_be_bytes()) {
        *slot ^= byte;
    }
    if let Some(version) = bytes.get_mut(6) {
        *version = (*version & 0x0F) | 0x40;
    }
    if let Some(variant) = bytes.get_mut(8) {
        *variant = (*variant & 0x3F) | 0x80;
    }
    Uuid::from_bytes(bytes)
}

const fn end_of<P>(ending: Ending) -> Response<P> {
    match ending {
        Ending::Completed => Response::NotFound,
        Ending::Failed(fault) => Response::Conflict(Conflict::Failed(fault)),
    }
}

const fn feeder_after(fault: Fault) -> AdfState {
    match fault {
        Fault::Jam => AdfState::Jam,
        Fault::DoubleFeed => AdfState::MultipickDetected,
        Fault::Scanner | Fault::Lost => AdfState::Loaded,
    }
}

impl<R, P, const N: usize> Bridge<R, P, N> {
    /// Builds an idle bridge that answers with `uuid` and derives job identifiers from `seed`.
    #[must_use]
    pub const fn new(uuid: Uuid, seed: [u8; 16]) -> Self {
        Self {
            uuid,
            seed,
            jobs: 0,
            job: None,
            hint: None,
        }
    }

    /// Decides the effects of one input at the moment `now`, after dropping state that expired before it.
    #[must_use]
    pub fn handle(self, input: Input<R, P>, now: Moment) -> (Self, Effects<R, P>) {
        let bridge = self.expire(now);
        match input {
            Input::Request { request, responder } => bridge.request(request, responder, now),
            Input::Reserved => bridge.reserved(),
            Input::Refused(refusal) => bridge.refused(refusal, now),
            Input::Page { side, page } => bridge.page(side, page),
            Input::Finished(ending) => bridge.finished(ending, now),
        }
    }

    fn expire(mut self, now: Moment) -> Self {
        if self.hint.is_some_and(|hint| now >= hint.until) {
            self.hint = None;
        }
        if self.job.as_ref().is_some_and(|job| {
            matches!(job.phase, Phase::Finished(_)) && now >= job.touched.after(RETENTION_MILLIS)
        }) {
            self.job = None;
        }
        self
    }

    fn request(self, request: Request, responder: R, now: Moment) -> (Self, Effects<R, P>) {
        let response = match request {
            Request::Capabilities => Response::Capabilities(Capabilities::new(self.uuid)),
            Request::Status => Response::Status(self.status()),
            Request::CreateJob(Ok(settings)) => return self.create(settings, responder, now),
            Request::CreateJob(Err(unsupported)) => {
                Response::Conflict(Conflict::Unsupported(unsupported))
            }
            Request::NextDocument(id) => return self.next_document(id, responder, now),
            Request::DeleteJob(id) => return self.delete(id, responder),
            Request::Unrouted(Unrouted::NotFound) => Response::NotFound,
            Request::Unrouted(Unrouted::MethodNotAllowed(allowed)) => {
                Response::MethodNotAllowed(allowed)
            }
            Request::Malformed => Response::BadRequest,
        };
        (self, Effects::reply(responder, response))
    }

    fn status(&self) -> ScannerStatus {
        let Some(job) = &self.job else {
            return ScannerStatus {
                state: ScannerState::Idle,
                adf: self.hint.map_or(AdfState::Loaded, |hint| hint.adf),
                job: None,
            };
        };
        let info = |state| {
            Some(JobInfo {
                id: job.id,
                images_completed: job.delivered,
                images_to_transfer: job.pages.count(),
                state,
            })
        };
        let (state, adf, job) = match &job.phase {
            Phase::Starting(_) => (ScannerState::Processing, AdfState::Loaded, None),
            Phase::Scanning { .. } => (
                ScannerState::Processing,
                AdfState::Loaded,
                info(JobState::Processing),
            ),
            Phase::Canceled => (
                ScannerState::Processing,
                AdfState::Loaded,
                info(JobState::Canceled),
            ),
            Phase::Finished(_) if !job.pages.is_empty() => (
                ScannerState::Processing,
                AdfState::Loaded,
                info(JobState::Processing),
            ),
            Phase::Finished(Ending::Completed) => (
                ScannerState::Idle,
                AdfState::Empty,
                info(JobState::Completed),
            ),
            Phase::Finished(Ending::Failed(fault)) => (
                ScannerState::Idle,
                feeder_after(*fault),
                info(JobState::Aborted),
            ),
        };
        ScannerStatus { state, adf, job }
    }

    fn create(mut self, settings: JobSettings, responder: R, now: Moment) -> (Self, Effects<R, P>) {
        let busy = self.job.as_ref().is_some_and(|job| match job.phase {
            Phase::Starting(_) | Phase::Scanning { .. } | Phase::Canceled => true,
            Phase::Finished(_) => !job.pages.is_empty(),
        });
        if busy {
            return (
                self,
                Effects::reply(responder, Response::Unavailable(Unavailable::JobInProgress)),
            );
        }
        let jobs = self.jobs.wrapping_add(1);
        self.job = Some(Job {
            id: job_id(self.seed, jobs),
            sides: settings.sides(),
            phase: Phase::Starting(responder),
            pages: Queue::new(),
            delivered: 0,
            lost: 0,
            touched: now,
        });
        self.jobs = jobs;
        self.hint = None;
        (self, Effects::one(Effect::Start(settings)))
    }

    fn reserved(mut self) -> (Self, Effects<R, P>) {
        let Some(job) = self.job.take() else {
            return (self, Effects::none());
        };
        let effects = match job.phase {
            Phase::Starting(responder) => {
                let id = job.id;
                self.job = Some(Job {
                    phase: Phase::Scanning { waiter: None },
                    ..job
                });
                Effects::reply(responder, Response::Created(JobPath::new(id)))
            }
            phase @ (Phase::Scanning { .. } | Phase::Canceled | Phase::Finished(_)) => {
                self.job = Some(Job { phase, ..job });
                Effects::none()
            }
        };
        (self, effects)
    }

    fn refused(mut self, refusal: Refusal, now: Moment) -> (Self, Effects<R, P>) {
        let Some(job) = self.job.take() else {
            return (self, Effects::none());
        };
        let effects = match job.phase {
            Phase::Starting(responder) => {
                let response = match refusal {
                    Refusal::NoPaper => {
                        self.hint = Some(Hint {
                            adf: AdfState::Empty,
                            until: now.after(HINT_MILLIS),
                        });
                        Response::Conflict(Conflict::NoPaper)
                    }
                    Refusal::Busy => Response::Unavailable(Unavailable::ScannerBusy),
                    Refusal::Unreachable => Response::Unavailable(Unavailable::ScannerUnreachable),
                };
                Effects::reply(responder, response)
            }
            phase @ (Phase::Scanning { .. } | Phase::Canceled | Phase::Finished(_)) => {
                self.job = Some(Job { phase, ..job });
                Effects::none()
            }
        };
        (self, effects)
    }

    fn page(mut self, side: Side, page: P) -> (Self, Effects<R, P>) {
        let Some(mut job) = self.job.take() else {
            return (self, Effects::none());
        };
        let wanted = !(job.sides == Sides::Front && side == Side::Back);
        let effects = match job.phase {
            Phase::Scanning {
                waiter: Some(responder),
            } if wanted => {
                job.delivered = job.delivered.saturating_add(1);
                self.job = Some(Job {
                    phase: Phase::Scanning { waiter: None },
                    ..job
                });
                Effects::reply(responder, Response::Page(page))
            }
            Phase::Scanning { waiter: None } if wanted => {
                if job.pages.push(page).is_err() {
                    job.lost = job.lost.saturating_add(1);
                }
                self.job = Some(Job {
                    phase: Phase::Scanning { waiter: None },
                    ..job
                });
                Effects::none()
            }
            phase @ (Phase::Starting(_)
            | Phase::Scanning { .. }
            | Phase::Canceled
            | Phase::Finished(_)) => {
                self.job = Some(Job { phase, ..job });
                Effects::none()
            }
        };
        (self, effects)
    }

    fn finished(mut self, ending: Ending, now: Moment) -> (Self, Effects<R, P>) {
        let Some(job) = self.job.take() else {
            return (self, Effects::none());
        };
        let effects = match job.phase {
            Phase::Scanning { waiter } => {
                let ending = match ending {
                    Ending::Completed if job.lost > 0 => Ending::Failed(Fault::Lost),
                    Ending::Completed | Ending::Failed(_) => ending,
                };
                self.job = Some(Job {
                    phase: Phase::Finished(ending),
                    touched: now,
                    ..job
                });
                waiter.map_or_else(Effects::none, |responder| {
                    Effects::reply(responder, end_of(ending))
                })
            }
            Phase::Starting(responder) => Effects::reply(
                responder,
                Response::Unavailable(Unavailable::ScannerUnreachable),
            ),
            Phase::Canceled => Effects::none(),
            phase @ Phase::Finished(_) => {
                self.job = Some(Job { phase, ..job });
                Effects::none()
            }
        };
        (self, effects)
    }

    fn next_document(mut self, id: Uuid, responder: R, now: Moment) -> (Self, Effects<R, P>) {
        let Some(mut job) = self.job.take() else {
            return (self, Effects::reply(responder, Response::NotFound));
        };
        if job.id != id {
            self.job = Some(job);
            return (self, Effects::reply(responder, Response::NotFound));
        }
        let effects = match job.phase {
            Phase::Scanning { waiter } => {
                job.touched = now;
                if let Some(page) = job.pages.pop() {
                    job.delivered = job.delivered.saturating_add(1);
                    self.job = Some(Job {
                        phase: Phase::Scanning { waiter },
                        ..job
                    });
                    Effects::reply(responder, Response::Page(page))
                } else {
                    self.job = Some(Job {
                        phase: Phase::Scanning {
                            waiter: Some(responder),
                        },
                        ..job
                    });
                    waiter.map_or_else(Effects::none, |older| {
                        Effects::reply(older, Response::Unavailable(Unavailable::Superseded))
                    })
                }
            }
            Phase::Finished(ending) => {
                job.touched = now;
                let response = job.pages.pop().map_or_else(
                    || end_of(ending),
                    |page| {
                        job.delivered = job.delivered.saturating_add(1);
                        Response::Page(page)
                    },
                );
                self.job = Some(Job {
                    phase: Phase::Finished(ending),
                    ..job
                });
                Effects::reply(responder, response)
            }
            phase @ (Phase::Starting(_) | Phase::Canceled) => {
                self.job = Some(Job { phase, ..job });
                Effects::reply(responder, Response::NotFound)
            }
        };
        (self, effects)
    }

    fn delete(mut self, id: Uuid, responder: R) -> (Self, Effects<R, P>) {
        let Some(job) = self.job.take() else {
            return (self, Effects::reply(responder, Response::NotFound));
        };
        if job.id != id {
            self.job = Some(job);
            return (self, Effects::reply(responder, Response::NotFound));
        }
        let effects = match job.phase {
            Phase::Scanning { waiter } => {
                self.job = Some(Job {
                    phase: Phase::Canceled,
                    pages: Queue::new(),
                    ..job
                });
                let deleted = Effect::Reply {
                    responder,
                    response: Response::Deleted,
                };
                Effects([
                    Some(deleted),
                    waiter.map(|waiting| Effect::Reply {
                        responder: waiting,
                        response: Response::NotFound,
                    }),
                ])
            }
            Phase::Finished(_) => Effects::reply(responder, Response::Deleted),
            phase @ (Phase::Starting(_) | Phase::Canceled) => {
                self.job = Some(Job { phase, ..job });
                Effects::reply(responder, Response::NotFound)
            }
        };
        (self, effects)
    }
}
