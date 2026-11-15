use vens_core::{
    FrameError,
    control::{
        Greeting, GreetingError, Release, ReleaseReply, Reserve, ReserveRefusal, ReserveReply,
    },
};

use crate::channel::Stream;

const GREETING_LENGTH: usize = 16;
const RESERVE_REPLY_LENGTH: usize = 20;
const RELEASE_REPLY_LENGTH: usize = 16;

#[derive(Debug, PartialEq, Eq)]
pub enum ControlError<E> {
    Port(E),
    Greeting(GreetingError),
    Frame(FrameError),
    Reserve(ReserveRefusal),
    Release(i32),
}

#[derive(Debug)]
pub struct ControlChannel<S> {
    stream: S,
}

impl<S: Stream> ControlChannel<S> {
    /// # Errors
    ///
    /// Fails when the stream fails or the scanner's greeting is not the accepted one.
    pub fn open(mut stream: S) -> Result<Self, ControlError<S::Error>> {
        let mut greeting = [0; GREETING_LENGTH];
        stream.receive(&mut greeting).map_err(ControlError::Port)?;
        Greeting::decode(&greeting).map_err(ControlError::Greeting)?;
        Ok(Self { stream })
    }

    /// # Errors
    ///
    /// Fails when the stream fails, the reply is malformed, or the scanner refuses the reserve.
    pub fn reserve(&mut self, reserve: Reserve) -> Result<(), ControlError<S::Error>> {
        self.stream
            .send(&reserve.encode())
            .map_err(ControlError::Port)?;
        let mut reply = [0; RESERVE_REPLY_LENGTH];
        self.stream
            .receive(&mut reply)
            .map_err(ControlError::Port)?;
        ReserveReply::decode(&reply)
            .map_err(ControlError::Frame)?
            .outcome()
            .map_err(ControlError::Reserve)
    }

    /// # Errors
    ///
    /// Fails when the stream fails, the reply is malformed, or the scanner refuses the release.
    pub fn release(&mut self, release: Release) -> Result<(), ControlError<S::Error>> {
        self.stream
            .send(&release.encode())
            .map_err(ControlError::Port)?;
        let mut reply = [0; RELEASE_REPLY_LENGTH];
        self.stream
            .receive(&mut reply)
            .map_err(ControlError::Port)?;
        ReleaseReply::decode(&reply)
            .map_err(ControlError::Frame)?
            .outcome()
            .map_err(ControlError::Release)
    }

    #[must_use]
    pub fn into_stream(self) -> S {
        self.stream
    }
}
