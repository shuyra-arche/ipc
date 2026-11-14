//! Task-local endpoints for bounded Clone queues.

mod payload;
mod reply;

pub use payload::Payload;
pub use reply::{Reply, ReplyError};

use core::marker::PhantomData;

use crate::view::memory::{MappingError, Memory};
use erra::Error;
use kernel_api::{
    abi::call::{clone_view::CloneFail, pend::PendFail},
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
    Mapping(MappingError),
    Return(Error<PendFail>),
    Capacity { required: usize },
    Returning,
}

/// Borrows the caller's pull authority and retains unfinished deliveries.
pub struct Receiver<M: Memory> {
    memory: M,
    token: PendId,
    pending: Option<Payload>,
    task: PhantomData<*mut ()>,
}

impl<M: Memory> Receiver<M> {
    pub fn new(token: PendId, memory: M) -> Self {
        Self {
            memory,
            token,
            pending: None,
            task: PhantomData,
        }
    }

    /// Borrow the Task context to copy or finish a manually pulled payload.
    pub fn memory(&mut self) -> &mut M {
        &mut self.memory
    }

    /// A failed Pull leaves the queue unchanged, including when max_len is too
    /// small. After a successful Pull, the caller owns the payload cleanup.
    /// A delivery retained after an unfinished FrameReceiver mapping is pulled first.
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

    /// Return a retained delivery without pulling or dispatching another frame.
    /// Use when abandoning an unfinished mapping or shutting down a service.
    /// Failure retains the delivery for retry. The borrowed queue stays open.
    pub fn discard(&mut self) -> Result<(), ReceiveError> {
        if let Some(payload) = self.pending.as_mut() {
            payload.retire(&mut self.memory)?;
            self.pending = None;
        }
        Ok(())
    }
}

impl<M: Memory> Drop for Receiver<M> {
    fn drop(&mut self) {
        let _ = self.discard();
    }
}

impl<M: Memory> FrameReceiver for Receiver<M> {
    type Error = ReceiveError;
    type Metadata = TaskId;

    /// A capacity failure retains the frame. Busy or uncertain mappings retain
    /// the delivery for another receive. Once copied, it is delivered even if
    /// Return needs more work. A later Pull settles that cleanup first.
    fn receive(&mut self, out: &mut [u8]) -> Result<ReceivedFrame<TaskId>, Self::Error> {
        let mut payload = self.pull(out.len())?;
        let metadata = payload.sender();
        let result = payload
            .copy(out, &mut self.memory)
            .map(|len| ReceivedFrame { len, metadata });
        self.pending = if matches!(&result, Err(ReceiveError::Mapping(error)) if error.is_busy() || matches!(error, MappingError::Transport(_) | MappingError::Uncertain { .. }))
        {
            Some(payload)
        } else {
            payload.finish(&mut self.memory).err().map(|_| payload)
        };
        result
    }
}
