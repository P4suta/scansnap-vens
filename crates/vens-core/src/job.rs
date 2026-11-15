use crate::{
    command::ScsiStatus,
    image::{Chunk, ChunkError, PixelSize, PixelSizeError},
    scan::{Command, Sequence, SheetIndex, Side},
    sense::{self, Condition, SenseError},
    status::{HardwareStatus, PaperPath, StatusError},
};

const NO_DATA_RETRIES: u8 = 100;
const BUSY_RETRIES: u8 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pause {
    NoDataYet,
    Busy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobError {
    NoPaper,
    UnsupportedPaperPath,
    Refused { status: u32 },
    SenseUnavailable,
    Condition(Condition),
    Status(StatusError),
    Sense(SenseError),
    Chunk(ChunkError),
    PixelSize(PixelSizeError),
    HeightOutOfRange { lines: u32 },
    RetriesExhausted(Pause),
    SequenceExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub sheets: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Setup,
    Running,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    CancelRead,
    PreRead,
    Parameters,
    Status,
    Start,
    NextSheet,
    Read { side: Side, sequence: Sequence },
    SenseAfterRead { side: Side, sequence: Sequence },
    SenseAfterNextSheet,
    SenseAfterRefusal,
    PixelSize { side: Side },
    EndJob { outcome: Result<(), JobError> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfterStore {
    NextRead { sequence: Sequence },
    Sense { sequence: Sequence },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Job {
    stage: Stage,
    phase: Phase,
    sheet: SheetIndex,
    sheets: u32,
    no_data: u8,
    busy: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Awaiting {
    job: Job,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paused {
    job: Job,
    pause: Pause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Storing<'reply> {
    job: Job,
    side: Side,
    data: &'reply [u8],
    then: AfterStore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finishing {
    job: Job,
    side: Side,
    height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step<'reply> {
    Execute(Awaiting),
    Pause(Paused),
    Store(Storing<'reply>),
    Finish(Finishing),
    Done(Summary),
    Failed(JobError),
}

#[must_use]
pub const fn start() -> Step<'static> {
    Step::Execute(Awaiting {
        job: Job {
            stage: Stage::Setup,
            phase: Phase::CancelRead,
            sheet: SheetIndex::FIRST,
            sheets: 0,
            no_data: 0,
            busy: 0,
        },
    })
}

impl Job {
    const fn execute(self, phase: Phase) -> Step<'static> {
        Step::Execute(Awaiting {
            job: Self { phase, ..self },
        })
    }

    const fn conclude(self, outcome: Result<(), JobError>) -> Step<'static> {
        match outcome {
            Ok(()) => Step::Done(Summary {
                sheets: self.sheets,
            }),
            Err(error) => Step::Failed(error),
        }
    }

    const fn fail(self, error: JobError) -> Step<'static> {
        match (self.phase, self.stage) {
            (
                Phase::EndJob {
                    outcome: Err(first),
                },
                _,
            ) => Step::Failed(first),
            (Phase::EndJob { outcome: Ok(()) }, _) | (_, Stage::Setup) => Step::Failed(error),
            (_, Stage::Running) => self.execute(Phase::EndJob {
                outcome: Err(error),
            }),
        }
    }

    const fn pause_for_busy(self) -> Step<'static> {
        if self.busy >= BUSY_RETRIES {
            return self.fail(JobError::RetriesExhausted(Pause::Busy));
        }
        Step::Pause(Paused {
            job: Self {
                busy: self.busy.saturating_add(1),
                ..self
            },
            pause: Pause::Busy,
        })
    }

    const fn pause_for_data(self, side: Side, sequence: Sequence) -> Step<'static> {
        if self.no_data >= NO_DATA_RETRIES {
            return self.fail(JobError::RetriesExhausted(Pause::NoDataYet));
        }
        Step::Pause(Paused {
            job: Self {
                no_data: self.no_data.saturating_add(1),
                phase: Phase::Read { side, sequence },
                ..self
            },
            pause: Pause::NoDataYet,
        })
    }

    fn next_read(self, side: Side, sequence: Sequence) -> Step<'static> {
        match sequence.next() {
            Some(sequence) => self.execute(Phase::Read { side, sequence }),
            None => self.fail(JobError::SequenceExhausted),
        }
    }

    fn after_status(self, data: &[u8]) -> Step<'static> {
        match HardwareStatus::decode(data).map(|status| status.paper()) {
            Ok(PaperPath::TopChute) => self.execute(Phase::Start),
            Ok(PaperPath::FrontSlot) => self.fail(JobError::UnsupportedPaperPath),
            Ok(PaperPath::Absent) => self.fail(JobError::NoPaper),
            Err(error) => self.fail(JobError::Status(error)),
        }
    }

    fn after_read(self, data: &[u8], side: Side, sequence: Sequence) -> Step<'_> {
        match Chunk::split(data) {
            Ok(chunk) if chunk.data.is_empty() => self.next_read(side, sequence),
            Ok(chunk) => Step::Store(Storing {
                job: Self { no_data: 0, ..self },
                side,
                data: chunk.data,
                then: AfterStore::NextRead { sequence },
            }),
            Err(error) => self.fail(JobError::Chunk(error)),
        }
    }

    fn after_read_sense(self, data: &[u8], side: Side, sequence: Sequence) -> Step<'static> {
        let first_read_of_sheet = side == Side::Front && sequence == Sequence::FIRST;
        match sense::decode(data) {
            Ok(Condition::EndOfSide) => {
                Self { no_data: 0, ..self }.execute(Phase::PixelSize { side })
            }
            Ok(Condition::NoDataYet) => self.pause_for_data(side, sequence),
            Ok(Condition::NoPaper) if first_read_of_sheet => {
                self.execute(Phase::EndJob { outcome: Ok(()) })
            }
            Ok(condition) => self.fail(JobError::Condition(condition)),
            Err(error) => self.fail(JobError::Sense(error)),
        }
    }

    fn after_sheet_sense(self, data: &[u8]) -> Step<'static> {
        match sense::decode(data) {
            Ok(Condition::NoPaper) => self.execute(Phase::EndJob { outcome: Ok(()) }),
            Ok(condition) => self.fail(JobError::Condition(condition)),
            Err(error) => self.fail(JobError::Sense(error)),
        }
    }

    fn after_refusal_sense(self, data: &[u8]) -> Step<'static> {
        match sense::decode(data) {
            Ok(condition) => self.fail(JobError::Condition(condition)),
            Err(error) => self.fail(JobError::Sense(error)),
        }
    }

    fn after_pixel_size(self, data: &[u8], side: Side) -> Step<'static> {
        match PixelSize::decode(data).map(|size| size.real_height()) {
            Ok(lines) => match u16::try_from(lines) {
                Ok(height) => Step::Finish(Finishing {
                    job: self,
                    side,
                    height,
                }),
                Err(_) => self.fail(JobError::HeightOutOfRange { lines }),
            },
            Err(error) => self.fail(JobError::PixelSize(error)),
        }
    }

    fn good(self, data: &[u8]) -> Step<'_> {
        match self.phase {
            Phase::CancelRead => self.execute(Phase::PreRead),
            Phase::PreRead => self.execute(Phase::Parameters),
            Phase::Parameters => self.execute(Phase::Status),
            Phase::Status => self.after_status(data),
            Phase::Start => Self {
                stage: Stage::Running,
                ..self
            }
            .execute(Phase::NextSheet),
            Phase::NextSheet => self.execute(Phase::Read {
                side: Side::Front,
                sequence: Sequence::FIRST,
            }),
            Phase::Read { side, sequence } => self.after_read(data, side, sequence),
            Phase::SenseAfterRead { side, sequence } => self.after_read_sense(data, side, sequence),
            Phase::SenseAfterNextSheet => self.after_sheet_sense(data),
            Phase::SenseAfterRefusal => self.after_refusal_sense(data),
            Phase::PixelSize { side } => self.after_pixel_size(data, side),
            Phase::EndJob { outcome } => self.conclude(outcome),
        }
    }

    fn check(self, data: &[u8]) -> Step<'_> {
        match self.phase {
            Phase::NextSheet => self.execute(Phase::SenseAfterNextSheet),
            Phase::Read { side, sequence } => match Chunk::split(data) {
                Ok(chunk) if !chunk.data.is_empty() => Step::Store(Storing {
                    job: Self { no_data: 0, ..self },
                    side,
                    data: chunk.data,
                    then: AfterStore::Sense { sequence },
                }),
                Ok(_) | Err(_) => self.execute(Phase::SenseAfterRead { side, sequence }),
            },
            Phase::SenseAfterRead { .. }
            | Phase::SenseAfterNextSheet
            | Phase::SenseAfterRefusal => self.fail(JobError::SenseUnavailable),
            Phase::EndJob { outcome } => self.conclude(outcome),
            Phase::CancelRead
            | Phase::PreRead
            | Phase::Parameters
            | Phase::Status
            | Phase::Start
            | Phase::PixelSize { .. } => self.execute(Phase::SenseAfterRefusal),
        }
    }
}

impl Awaiting {
    #[must_use]
    pub fn command(&self) -> Command {
        let sheet = self.job.sheet;
        match self.job.phase {
            Phase::CancelRead => Command::cancel_read(),
            Phase::PreRead => Command::pre_read(),
            Phase::Parameters => Command::scan_parameters(),
            Phase::Status => Command::hardware_status(),
            Phase::Start => Command::start_job(),
            Phase::NextSheet => Command::next_sheet(),
            Phase::Read { side, sequence } => Command::read_image(side, sheet, sequence),
            Phase::SenseAfterRead { .. }
            | Phase::SenseAfterNextSheet
            | Phase::SenseAfterRefusal => Command::request_sense(),
            Phase::PixelSize { side } => Command::read_pixel_size(side, sheet),
            Phase::EndJob { .. } => Command::end_job(),
        }
    }

    #[must_use]
    pub fn replied(self, status: ScsiStatus, data: &[u8]) -> Step<'_> {
        let job = self.job;
        match status {
            ScsiStatus::Busy => job.pause_for_busy(),
            ScsiStatus::Unrecognized(status) => job.fail(JobError::Refused { status }),
            ScsiStatus::Good => Job { busy: 0, ..job }.good(data),
            ScsiStatus::CheckCondition => Job { busy: 0, ..job }.check(data),
        }
    }
}

impl Paused {
    #[must_use]
    pub const fn pause(&self) -> Pause {
        self.pause
    }

    #[must_use]
    pub const fn resumed(self) -> Step<'static> {
        Step::Execute(Awaiting { job: self.job })
    }
}

impl<'reply> Storing<'reply> {
    #[must_use]
    pub const fn side(&self) -> Side {
        self.side
    }

    #[must_use]
    pub const fn sheet_number(&self) -> u32 {
        self.job.sheets
    }

    #[must_use]
    pub const fn data(&self) -> &'reply [u8] {
        self.data
    }

    #[must_use]
    pub fn stored(self) -> Step<'static> {
        match self.then {
            AfterStore::NextRead { sequence } => self.job.next_read(self.side, sequence),
            AfterStore::Sense { sequence } => self.job.execute(Phase::SenseAfterRead {
                side: self.side,
                sequence,
            }),
        }
    }
}

impl Finishing {
    #[must_use]
    pub const fn side(&self) -> Side {
        self.side
    }

    #[must_use]
    pub const fn sheet_number(&self) -> u32 {
        self.job.sheets
    }

    #[must_use]
    pub const fn height(&self) -> u16 {
        self.height
    }

    #[must_use]
    pub const fn finished(self) -> Step<'static> {
        match self.side {
            Side::Front => self.job.execute(Phase::Read {
                side: Side::Back,
                sequence: Sequence::FIRST,
            }),
            Side::Back => Job {
                sheet: self.job.sheet.next(),
                sheets: self.job.sheets.saturating_add(1),
                ..self.job
            }
            .execute(Phase::NextSheet),
        }
    }
}
