use core::mem;

use crate::{
    document::{AdfState, Capabilities, JobInfo, JobState, ScannerState, ScannerStatus},
    route::{JobPath, Method, Unrouted},
    settings::{JobSettings, Sides, Unsupported},
    uuid::Uuid,
};

/// How long the feeder state that a refused job request or a drained job shows stays in the status.
///
/// sane-airscan checks the status before each job, so a longer stay would refuse the next scan.
pub const HINT_MILLIS: u64 = 10_000;
/// How long a finished job with pending pages stays after its end or its last page request, whichever came later.
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
    /// The scanner has no registered host, which one pairing with the vendor software fixes.
    Unregistered,
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
    Abandoned,
    Refused(Refusal),
    Page { side: Side, page: P },
    PageWritten { id: Uuid, returned: Option<P> },
    Observed { id: Uuid, observation: Observation },
    Finished(Ending),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conflict {
    Unsupported(Unsupported),
    NoPaper,
    Unregistered,
    Failed(Fault),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observation {
    NoPaper,
    Fault(Fault),
}

impl Observation {
    #[must_use]
    pub const fn conflict(self) -> Conflict {
        match self {
            Self::NoPaper => Conflict::NoPaper,
            Self::Fault(fault) => Conflict::Failed(fault),
        }
    }
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
    Transfer {
        responder: R,
        id: Uuid,
        page: P,
    },
    Observation {
        responder: R,
        id: Uuid,
        observation: Observation,
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

    const fn transfer(responder: R, id: Uuid, page: P) -> Self {
        Self::one(Effect::Transfer {
            responder,
            id,
            page,
        })
    }

    const fn end(responder: R, id: Uuid, ending: Ending) -> Self {
        match ending {
            Ending::Completed => Self::reply(responder, Response::NotFound),
            Ending::Failed(fault) => Self::one(Effect::Observation {
                responder,
                id,
                observation: Observation::Fault(fault),
            }),
        }
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
enum Transfer<P> {
    Idle,
    Writing,
    Retry(P),
}

#[derive(Debug, Clone)]
struct Job<R, P, const N: usize> {
    id: Uuid,
    sides: Sides,
    phase: Phase<R>,
    pages: Queue<P, N>,
    transfer: Transfer<P>,
    delivered: u32,
    lost: u32,
    fault_until: Option<Moment>,
    touched: Moment,
}

impl<R, P, const N: usize> Job<R, P, N> {
    const fn has_pages(&self) -> bool {
        !self.pages.is_empty() || !matches!(self.transfer, Transfer::Idle)
    }

    fn pending(&self) -> u32 {
        let assigned = u32::from(!matches!(self.transfer, Transfer::Idle));
        self.pages.count().saturating_add(assigned)
    }

    fn take_page(&mut self) -> Option<P> {
        let page = match mem::replace(&mut self.transfer, Transfer::Idle) {
            Transfer::Retry(page) => Some(page),
            Transfer::Idle => self.pages.pop(),
            Transfer::Writing => {
                self.transfer = Transfer::Writing;
                None
            }
        };
        if page.is_some() {
            self.transfer = Transfer::Writing;
        }
        page
    }
}

#[derive(Debug, Clone, Copy)]
struct Hint {
    adf: AdfState,
    until: Moment,
}

/// Waits between jobs with the feeder state that a refusal found, or runs one job.
#[derive(Debug, Clone)]
enum Activity<R, P, const N: usize> {
    Idle(Option<Hint>),
    Busy(Job<R, P, N>),
}

/// The state of a bridge that runs one job at a time and keeps up to `N` pages for its client.
#[derive(Debug, Clone)]
pub struct Bridge<R, P, const N: usize> {
    uuid: Uuid,
    seed: [u8; 16],
    jobs: u32,
    activity: Activity<R, P, N>,
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
            activity: Activity::Idle(None),
        }
    }

    /// Takes the running job out of the state, and leaves an idle state with any hint in place.
    fn take_job(&mut self) -> Option<Job<R, P, N>> {
        match mem::replace(&mut self.activity, Activity::Idle(None)) {
            Activity::Busy(job) => Some(job),
            idle @ Activity::Idle(_) => {
                self.activity = idle;
                None
            }
        }
    }

    fn keep(&mut self, job: Job<R, P, N>) {
        self.activity = Activity::Busy(job);
    }

    /// Decides the effects of one input at the moment `now`, after dropping state that expired before it.
    #[must_use]
    pub fn handle(self, input: Input<R, P>, now: Moment) -> (Self, Effects<R, P>) {
        let bridge = self.expire(now);
        match input {
            Input::Request { request, responder } => bridge.request(request, responder, now),
            Input::Reserved => bridge.reserved(),
            Input::Abandoned => bridge.abandoned(),
            Input::Refused(refusal) => bridge.refused(refusal),
            Input::Page { side, page } => bridge.page(side, page),
            Input::PageWritten { id, returned } => {
                (bridge.page_written(id, returned, now), Effects::none())
            }
            Input::Observed { id, observation } => {
                (bridge.observed(id, observation, now), Effects::none())
            }
            Input::Finished(ending) => bridge.finished(ending, now),
        }
    }

    fn expire(mut self, now: Moment) -> Self {
        let expired = match &self.activity {
            Activity::Idle(Some(hint)) => now >= hint.until,
            Activity::Idle(None) => false,
            Activity::Busy(job) => match job.phase {
                Phase::Finished(Ending::Failed(_)) if !job.has_pages() => {
                    now >= job
                        .fault_until
                        .unwrap_or_else(|| job.touched.after(RETENTION_MILLIS))
                }
                Phase::Finished(_) => {
                    let stay = if matches!(job.transfer, Transfer::Writing) {
                        return self;
                    } else if !job.has_pages() {
                        HINT_MILLIS
                    } else {
                        RETENTION_MILLIS
                    };
                    now >= job.touched.after(stay)
                }
                Phase::Starting(_) | Phase::Scanning { .. } | Phase::Canceled => false,
            },
        };
        if expired {
            self.activity = Activity::Idle(None);
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
        let job = match &self.activity {
            Activity::Idle(hint) => {
                return ScannerStatus {
                    state: ScannerState::Idle,
                    adf: hint.map_or(AdfState::Loaded, |hint| hint.adf),
                    job: None,
                };
            }
            Activity::Busy(job) => job,
        };
        let info = |state| {
            Some(JobInfo {
                id: job.id,
                images_completed: job.delivered,
                images_to_transfer: job.pending(),
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
            Phase::Finished(_) if job.has_pages() => (
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
        let busy = match &self.activity {
            Activity::Idle(_) => false,
            Activity::Busy(job) => match job.phase {
                Phase::Starting(_) | Phase::Scanning { .. } | Phase::Canceled => true,
                Phase::Finished(_) => job.has_pages(),
            },
        };
        if busy {
            return (
                self,
                Effects::reply(responder, Response::Unavailable(Unavailable::JobInProgress)),
            );
        }
        let jobs = self.jobs.wrapping_add(1);
        self.keep(Job {
            id: job_id(self.seed, jobs),
            sides: settings.sides(),
            phase: Phase::Starting(responder),
            pages: Queue::new(),
            transfer: Transfer::Idle,
            delivered: 0,
            lost: 0,
            fault_until: None,
            touched: now,
        });
        self.jobs = jobs;
        (self, Effects::one(Effect::Start(settings)))
    }

    fn reserved(mut self) -> (Self, Effects<R, P>) {
        let Some(job) = self.take_job() else {
            return (self, Effects::none());
        };
        let effects = match job.phase {
            Phase::Starting(responder) => {
                let id = job.id;
                self.keep(Job {
                    phase: Phase::Scanning { waiter: None },
                    ..job
                });
                Effects::reply(responder, Response::Created(JobPath::new(id)))
            }
            phase @ (Phase::Scanning { .. } | Phase::Canceled | Phase::Finished(_)) => {
                self.keep(Job { phase, ..job });
                Effects::none()
            }
        };
        (self, effects)
    }

    fn refused(mut self, refusal: Refusal) -> (Self, Effects<R, P>) {
        let Some(job) = self.take_job() else {
            return (self, Effects::none());
        };
        let effects = match job.phase {
            Phase::Starting(responder) => {
                let response = match refusal {
                    Refusal::NoPaper => {
                        return (
                            self,
                            Effects::one(Effect::Observation {
                                responder,
                                id: job.id,
                                observation: Observation::NoPaper,
                            }),
                        );
                    }
                    Refusal::Unregistered => Response::Conflict(Conflict::Unregistered),
                    Refusal::Busy => Response::Unavailable(Unavailable::ScannerBusy),
                    Refusal::Unreachable => Response::Unavailable(Unavailable::ScannerUnreachable),
                };
                Effects::reply(responder, response)
            }
            phase @ (Phase::Scanning { .. } | Phase::Canceled | Phase::Finished(_)) => {
                self.keep(Job { phase, ..job });
                Effects::none()
            }
        };
        (self, effects)
    }

    fn abandoned(mut self) -> (Self, Effects<R, P>) {
        let effects = self
            .take_job()
            .map_or_else(Effects::none, |job| match job.phase {
                Phase::Starting(_) | Phase::Canceled => Effects::none(),
                Phase::Scanning { waiter } => waiter.map_or_else(Effects::none, |waiter| {
                    Effects::reply(waiter, Response::NotFound)
                }),
                phase @ Phase::Finished(_) => {
                    self.keep(Job { phase, ..job });
                    Effects::none()
                }
            });
        (self, effects)
    }

    fn page(mut self, side: Side, page: P) -> (Self, Effects<R, P>) {
        let Some(mut job) = self.take_job() else {
            return (self, Effects::none());
        };
        let wanted = !(job.sides == Sides::Front && side == Side::Back);
        let effects = match job.phase {
            Phase::Scanning {
                waiter: Some(responder),
            } if wanted => {
                let id = job.id;
                self.keep(Job {
                    phase: Phase::Scanning { waiter: None },
                    transfer: Transfer::Writing,
                    ..job
                });
                Effects::transfer(responder, id, page)
            }
            Phase::Scanning { waiter: None } if wanted => {
                if job.pages.push(page).is_err() {
                    job.lost = job.lost.saturating_add(1);
                }
                self.keep(Job {
                    phase: Phase::Scanning { waiter: None },
                    ..job
                });
                Effects::none()
            }
            phase @ (Phase::Starting(_)
            | Phase::Scanning { .. }
            | Phase::Canceled
            | Phase::Finished(_)) => {
                self.keep(Job { phase, ..job });
                Effects::none()
            }
        };
        (self, effects)
    }

    fn page_written(mut self, id: Uuid, returned: Option<P>, now: Moment) -> Self {
        if let Activity::Busy(job) = &mut self.activity
            && job.id == id
            && matches!(job.transfer, Transfer::Writing)
        {
            job.transfer = if let Some(page) = returned {
                Transfer::Retry(page)
            } else {
                job.delivered = job.delivered.saturating_add(1);
                Transfer::Idle
            };
            job.touched = now;
        }
        self
    }

    fn observed(mut self, id: Uuid, observation: Observation, now: Moment) -> Self {
        match observation {
            Observation::NoPaper => {
                if matches!(self.activity, Activity::Idle(None))
                    && job_id(self.seed, self.jobs) == id
                {
                    self.activity = Activity::Idle(Some(Hint {
                        adf: AdfState::Empty,
                        until: now.after(HINT_MILLIS),
                    }));
                }
            }
            Observation::Fault(fault) => {
                if let Activity::Busy(job) = &mut self.activity
                    && job.id == id
                    && matches!(job.phase, Phase::Finished(Ending::Failed(ended)) if ended == fault)
                    && !job.has_pages()
                    && job.fault_until.is_none()
                {
                    job.fault_until = Some(now.after(HINT_MILLIS));
                }
            }
        }
        self
    }

    fn finished(mut self, ending: Ending, now: Moment) -> (Self, Effects<R, P>) {
        let Some(job) = self.take_job() else {
            return (self, Effects::none());
        };
        let effects = match job.phase {
            Phase::Scanning { waiter } => {
                let id = job.id;
                let ending = match ending {
                    Ending::Completed if job.lost > 0 => Ending::Failed(Fault::Lost),
                    Ending::Completed | Ending::Failed(_) => ending,
                };
                self.keep(Job {
                    phase: Phase::Finished(ending),
                    touched: now,
                    ..job
                });
                waiter.map_or_else(Effects::none, |responder| {
                    Effects::end(responder, id, ending)
                })
            }
            Phase::Starting(responder) => Effects::reply(
                responder,
                Response::Unavailable(Unavailable::ScannerUnreachable),
            ),
            Phase::Canceled => Effects::none(),
            phase @ Phase::Finished(_) => {
                self.keep(Job { phase, ..job });
                Effects::none()
            }
        };
        (self, effects)
    }

    fn next_document(mut self, id: Uuid, responder: R, now: Moment) -> (Self, Effects<R, P>) {
        let Some(mut job) = self.take_job() else {
            return (self, Effects::reply(responder, Response::NotFound));
        };
        if job.id != id {
            self.keep(job);
            return (self, Effects::reply(responder, Response::NotFound));
        }
        let page = match job.phase {
            Phase::Scanning { .. } | Phase::Finished(_) => {
                if matches!(job.transfer, Transfer::Writing) {
                    self.keep(job);
                    return (
                        self,
                        Effects::reply(
                            responder,
                            Response::Unavailable(Unavailable::JobInProgress),
                        ),
                    );
                }
                job.take_page()
            }
            Phase::Starting(_) | Phase::Canceled => None,
        };
        let effects = match job.phase {
            Phase::Scanning { waiter } => {
                job.touched = now;
                if let Some(page) = page {
                    self.keep(Job {
                        phase: Phase::Scanning { waiter },
                        ..job
                    });
                    Effects::transfer(responder, id, page)
                } else {
                    self.keep(Job {
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
                if page.is_some() || ending == Ending::Completed {
                    job.touched = now;
                }
                let effects = match page {
                    Some(page) => Effects::transfer(responder, id, page),
                    None => Effects::end(responder, id, ending),
                };
                self.keep(Job {
                    phase: Phase::Finished(ending),
                    ..job
                });
                effects
            }
            phase @ (Phase::Starting(_) | Phase::Canceled) => {
                self.keep(Job { phase, ..job });
                Effects::reply(responder, Response::NotFound)
            }
        };
        (self, effects)
    }

    fn delete(mut self, id: Uuid, responder: R) -> (Self, Effects<R, P>) {
        let Some(job) = self.take_job() else {
            return (self, Effects::reply(responder, Response::NotFound));
        };
        if job.id != id {
            self.keep(job);
            return (self, Effects::reply(responder, Response::NotFound));
        }
        let effects = match job.phase {
            Phase::Scanning { waiter } => {
                self.keep(Job {
                    phase: Phase::Canceled,
                    pages: Queue::new(),
                    transfer: Transfer::Idle,
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
                self.keep(Job { phase, ..job });
                Effects::reply(responder, Response::NotFound)
            }
        };
        (self, effects)
    }
}
