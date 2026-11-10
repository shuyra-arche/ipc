//! Task-local endpoints for bounded Clone queues.

mod payload;
mod reply;

pub use payload::Payload;
pub use reply::{Reply, ReplyError};

use core::marker::PhantomData;

use erra::Error;
use kernel_api::{
    abi::call::{clone_view::CloneFail, pend::PendFail, share_view::ShareFail},
    address::VirtAddr,
    model::{
        clone_view::CloneCondition,
        identity::{PendId, TaskId},
        sign::SignSource,
    },
};
use kernel_client::call::clone_view;
use weave::{FrameReceiver, FrameSender, ReceivedFrame};

/// Borrows the caller's push authority. The Kernel checks it on each call.
pub struct Sender {
    token: PendId,
    task: PhantomData<*mut ()>,
}

impl Sender {
    pub fn new(token: PendId) -> Self {
        Self {
            token,
            task: PhantomData,
        }
    }

    /// Submit one nonempty message. Success means the queue accepted it.
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), Error<CloneFail>> {
        clone_view::push(
            self.token,
            VirtAddr::new(bytes.as_ptr().addr()),
            bytes.len(),
        )
    }

    /// Observing push readiness also requires PEEK on the endpoint. Readiness
    /// means there is some queue space, which may be smaller than the frame.
    pub fn source(&self) -> SignSource {
        SignSource::Clone {
            token: self.token,
            condition: CloneCondition::Push,
        }
    }
}

impl FrameSender for Sender {
    type Error = Error<CloneFail>;

    fn send(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.push(bytes)
    }
}

#[derive(Debug)]
pub enum ReceiveError {
    Pull(Error<CloneFail>),
    Open(Error<ShareFail>),
    Shut(Error<ShareFail>),
    Return(Error<PendFail>),
    Capacity { required: usize },
    InvalidMapping,
    Returning,
}

/// Borrows the caller's pull authority and retains unfinished deliveries.
pub struct Receiver {
    token: PendId,
    pending: Option<Payload>,
    task: PhantomData<*mut ()>,
}

impl Receiver {
    pub fn new(token: PendId) -> Self {
        Self {
            token,
            pending: None,
            task: PhantomData,
        }
    }

    /// A failed Pull leaves the queue unchanged, including when max_len is too
    /// small. After a successful Pull, the caller owns the payload cleanup.
    /// A delivery retained after a busy FrameReceiver Open is pulled first.
    pub fn pull(&mut self, max_len: usize) -> Result<Payload, ReceiveError> {
        self.settle()?;
        if let Some(payload) = self.pending.take() {
            let required = payload.len();
            if required > max_len {
                self.pending = Some(payload);
                return Err(ReceiveError::Capacity { required });
            }
            return Ok(payload);
        }
        clone_view::pull(self.token, max_len)
            .map(Payload::new)
            .map_err(ReceiveError::Pull)
    }

    /// Observe the Kernel queue's length, sender and depth without reserving a
    /// message. A delivery already pulled by this adapter is outside the queue.
    pub fn peek(&self) -> Result<(usize, TaskId, usize), Error<CloneFail>> {
        clone_view::peek(self.token)
    }

    pub fn source(&self) -> SignSource {
        SignSource::Clone {
            token: self.token,
            condition: CloneCondition::Pull,
        }
    }

    /// Retry cleanup of the previous consumed frame without pulling another.
    pub fn settle(&mut self) -> Result<(), ReceiveError> {
        if self.pending.as_ref().is_some_and(Payload::returning) {
            self.discard()?;
        }
        Ok(())
    }

    fn discard(&mut self) -> Result<(), ReceiveError> {
        if let Some(payload) = self.pending.as_mut() {
            payload.retire()?;
            self.pending = None;
        }
        Ok(())
    }
}

impl FrameReceiver for Receiver {
    type Error = ReceiveError;
    type Metadata = TaskId;

    /// A capacity failure retains the frame. A busy Open retains the delivery
    /// for another receive. Once a frame is copied, it is delivered even if
    /// Return needs more work. A later Pull settles that cleanup first.
    fn receive(&mut self, out: &mut [u8]) -> Result<ReceivedFrame<TaskId>, Self::Error> {
        let mut payload = self.pull(out.len())?;
        let metadata = payload.sender();
        let result = payload.copy(out).map(|len| ReceivedFrame { len, metadata });
        self.pending = if matches!(&result, Err(ReceiveError::Open(error)) if error.source.is_busy())
        {
            Some(payload)
        } else {
            payload.finish().err().map(|(payload, _)| payload)
        };
        result
    }
}
