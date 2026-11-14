//! One request's reply queue and restricted push authority.

use erra::Error;
use kernel_api::{
    abi::call::{clone_view::CloneFail, pend::PendFail},
    address::VirtAddr,
    model::{
        clone_view::CloneLimits,
        identity::{Mark, PendId, TaskId},
        modifiable::Modifiable,
        pend::{
            AccordRequest, AccordTarget, CloneRights, Distributable, Limits, Operations, PendInfo,
            PendScope, PendStatus,
        },
        sign::SignSource,
    },
};
use kernel_client::call::{clone_view, pend};
use weave::{FrameReceiver, ReceivedFrame, correlation::ReplyReceiver};

use super::{ReceiveError, Receiver, Sender};
use crate::view::memory::Memory;

const PUSH: Limits = Limits {
    operations: Operations::Clone(CloneRights::PUSH),
    modifiable: Modifiable::Yes,
    distributable: Distributable::No,
    scope: PendScope::Object,
};

#[derive(Debug)]
pub enum ReplyError {
    Accord(Error<PendFail>),
    Inspect(Error<PendFail>),
    Receive(ReceiveError),
    Return(Error<PendFail>),
    InvalidGrant,
    InvalidState,
    WrongSender { expected: TaskId, actual: TaskId },
}

enum State {
    Local,
    Granted(TaskId),
    Received,
    Returning,
    Finished,
}

/// A fresh single-message queue associated with exactly one request.
#[must_use = "finish the reply route to observe and retry cleanup failures"]
pub struct Reply<M: Memory> {
    receiver: Receiver<M>,
    state: State,
}

impl<M: Memory> Reply<M> {
    pub fn create(license: PendId, max_len: usize, memory: M) -> Result<Self, Error<CloneFail>> {
        let limits = CloneLimits {
            max_len,
            max_messages: 1,
            max_bytes: max_len,
        };
        let token = clone_view::create(license, Mark::NONE, limits)?;
        Ok(Self {
            receiver: Receiver::new(token, memory),
            state: State::Local,
        })
    }

    /// Grant one service push authority. The returned identifier is local
    /// to that service Task and can be carried in the request frame.
    pub fn grant(&mut self, service: TaskId) -> Result<PendId, ReplyError> {
        if !matches!(self.state, State::Local) {
            return Err(ReplyError::InvalidState);
        }
        let request = AccordRequest {
            src: self.receiver.token,
            target: AccordTarget::Task(service),
            limits: PUSH,
            mark: Mark::NONE,
        }
        .to_bytes();
        let token =
            pend::accord(VirtAddr::new(request.as_ptr().addr())).map_err(ReplyError::Accord)?;
        self.state = State::Granted(service);
        Ok(token)
    }

    /// The caller can attach this source before submitting the request.
    pub fn source(&self) -> SignSource {
        self.receiver.source()
    }

    /// Close the route after success, failure or timeout. Retry on a failed
    /// return. Closing the route does not cancel the service's work.
    pub fn finish(&mut self) -> Result<(), ReplyError> {
        if matches!(self.state, State::Finished) {
            return Ok(());
        }
        self.state = State::Returning;
        self.receiver.discard().map_err(ReplyError::Receive)?;
        pend::return_(self.receiver.token).map_err(ReplyError::Return)?;
        self.state = State::Finished;
        Ok(())
    }
}

impl<M: Memory> FrameReceiver for Reply<M> {
    type Error = ReplyError;
    type Metadata = TaskId;

    fn receive(&mut self, out: &mut [u8]) -> Result<ReceivedFrame<TaskId>, Self::Error> {
        let State::Granted(expected) = self.state else {
            return Err(ReplyError::InvalidState);
        };
        let frame = self.receiver.receive(out).map_err(ReplyError::Receive)?;
        if frame.metadata != expected {
            return Err(ReplyError::WrongSender {
                expected,
                actual: frame.metadata,
            });
        }
        self.state = State::Received;
        Ok(frame)
    }
}

impl<M: Memory> ReplyReceiver for Reply<M> {}

impl<M: Memory> Drop for Reply<M> {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

impl Sender {
    /// Resolve a reply grant named by a request. Use the sender recorded by
    /// Clone Pull as the requester. Kernel authorization is checked again at Push.
    pub fn reply(token: PendId, requester: TaskId) -> Result<Self, ReplyError> {
        let mut bytes = [0; PendInfo::SIZE];
        pend::inspect(token, VirtAddr::new(bytes.as_mut_ptr().addr()))
            .map_err(ReplyError::Inspect)?;
        let info = PendInfo::from_bytes(&bytes).ok_or(ReplyError::InvalidGrant)?;
        if info.token != token
            || info.owner != requester
            || info.lender != requester
            || info.limits != PUSH
            || info.status != PendStatus::Active
        {
            return Err(ReplyError::InvalidGrant);
        }
        Ok(Self::new(token))
    }
}
