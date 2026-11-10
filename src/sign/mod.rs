//! Dedicated Sign watches and coalesced direct wakeups.

use core::marker::PhantomData;

use erra::Error;
use kernel_api::{
    abi::call::{
        pend::PendFail,
        sign::{SignFail, SignReply},
    },
    model::{identity::PendId, sign::SignSource, wait::Wait},
};
use kernel_client::call::{pend, sign};

#[derive(Debug)]
pub enum WatchError {
    Sign(Error<SignFail>),
    InvalidState,
}

enum State {
    Idle,
    Attached(SignSource),
    Returning,
    Finished,
}

/// Owns a dedicated Sign with at most one attached source.
#[must_use = "finish the watch to observe and retry cleanup failures"]
pub struct Watch {
    token: PendId,
    state: State,
    task: PhantomData<*mut ()>,
}

impl Watch {
    pub fn create(license: PendId) -> Result<Self, Error<SignFail>> {
        Ok(Self {
            token: sign::create(license)?,
            state: State::Idle,
            task: PhantomData,
        })
    }

    /// Use generation zero for Clone and Join sources.
    pub fn attach(&mut self, source: SignSource, generation: u64) -> Result<(), WatchError> {
        if !matches!(self.state, State::Idle) {
            return Err(WatchError::InvalidState);
        }
        sign::attach(self.token, source, generation).map_err(WatchError::Sign)?;
        self.state = State::Attached(source);
        Ok(())
    }

    /// An event reports readiness, source failure or timeout. Recheck the
    /// actual operation after waking to account for competing consumers.
    pub fn listen(&mut self, wait: Wait) -> Result<SignReply, WatchError> {
        if !matches!(self.state, State::Attached(_)) {
            return Err(WatchError::InvalidState);
        }
        sign::listen(self.token, wait).map_err(WatchError::Sign)
    }

    pub fn detach(&mut self) -> Result<(), WatchError> {
        let State::Attached(source) = self.state else {
            return Err(WatchError::InvalidState);
        };
        sign::detach(self.token, source).map_err(WatchError::Sign)?;
        self.state = State::Idle;
        Ok(())
    }

    /// Return the Sign and remove its route, including when still attached.
    pub fn finish(&mut self) -> Result<(), Error<PendFail>> {
        if !matches!(self.state, State::Finished) {
            self.state = State::Returning;
            pend::return_(self.token)?;
            self.state = State::Finished;
        }
        Ok(())
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

/// Set a direct readiness notification. One already-pending trigger is
/// sufficient because the receiver rechecks its actual readiness condition.
pub fn trigger(token: PendId) -> Result<(), Error<SignFail>> {
    match sign::trigger(token) {
        Err(error) if error.source.is_busy() => Ok(()),
        result => result,
    }
}
