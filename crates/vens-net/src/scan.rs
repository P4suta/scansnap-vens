use vens_core::{
    job::{JobError, Pause, Step, Summary, start},
    scan::Side,
};

use crate::channel::{ChannelError, CommandChannel, Stream};

pub trait Clock {
    fn wait(&mut self, pause: Pause);
}

pub trait PageSink {
    type Error;

    /// # Errors
    ///
    /// Reports the sink's own failure to keep the bytes.
    fn append(&mut self, sheet: u32, side: Side, data: &[u8]) -> Result<(), Self::Error>;

    /// # Errors
    ///
    /// Reports the sink's own failure to complete the side.
    fn finish(&mut self, sheet: u32, side: Side, height: u16) -> Result<(), Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum ScanError<P, K> {
    Channel(ChannelError<P>),
    Sink(K),
    Job(JobError),
}

/// Runs one scan job to its end and hands every image byte to the sink.
///
/// # Errors
///
/// Fails when the command channel fails, the sink fails, or the job ends with an error.
pub fn run<S: Stream, C: Clock, K: PageSink>(
    channel: &mut CommandChannel<S>,
    clock: &mut C,
    sink: &mut K,
    buffer: &mut [u8],
) -> Result<Summary, ScanError<S::Error, K::Error>> {
    let mut step = start();
    loop {
        step = match step {
            Step::Execute(awaiting) => {
                let reply = channel
                    .execute(&awaiting.command(), buffer)
                    .map_err(ScanError::Channel)?;
                awaiting.replied(reply.status, reply.data)
            }
            Step::Pause(paused) => {
                clock.wait(paused.pause());
                paused.resumed()
            }
            Step::Store(storing) => {
                sink.append(storing.sheet_number(), storing.side(), storing.data())
                    .map_err(ScanError::Sink)?;
                storing.stored()
            }
            Step::Finish(finishing) => {
                sink.finish(
                    finishing.sheet_number(),
                    finishing.side(),
                    finishing.height(),
                )
                .map_err(ScanError::Sink)?;
                finishing.finished()
            }
            Step::Done(summary) => return Ok(summary),
            Step::Failed(error) => return Err(ScanError::Job(error)),
        };
    }
}
