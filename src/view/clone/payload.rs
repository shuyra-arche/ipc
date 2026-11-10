//! Independent read-only deliveries and their local mapping lifecycle.

use core::{marker::PhantomData, ptr};

use kernel_api::model::{clone_view::PullOutcome, identity::TaskId};
use kernel_client::call::{pend, share_view};

use super::ReceiveError;

#[derive(Debug)]
enum State {
    Held,
    Open,
    Shutting,
    Returning,
    Finished,
}

/// One successful Clone Pull. Finish it explicitly to observe cleanup failures.
#[derive(Debug)]
#[must_use = "finish the payload to observe and retry cleanup failures"]
pub struct Payload {
    delivery: PullOutcome,
    state: State,
    task: PhantomData<*mut ()>,
}

impl Payload {
    pub(super) fn new(delivery: PullOutcome) -> Self {
        Self {
            delivery,
            state: State::Held,
            task: PhantomData,
        }
    }

    pub fn sender(&self) -> TaskId {
        self.delivery.sender
    }

    pub fn len(&self) -> usize {
        self.delivery.len
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Copy one complete payload. The mapping stays open until Shut or Return.
    /// Keeping the read inside this operation avoids exposing a Rust slice
    /// across later calls that could withdraw the mapping.
    pub fn copy(&mut self, out: &mut [u8]) -> Result<usize, ReceiveError> {
        let len = self.len();
        if out.len() < len {
            return Err(ReceiveError::Capacity { required: len });
        }
        let address = self.open()?;
        // SAFETY: Clone Pull owns immutable copied cells under a task-local
        // root. No caller code or Kernel call runs between Open and this copy.
        // The destination is a writable Rust buffer and cannot overlap those
        // read-only cells. Open checked that the mapping covers len bytes.
        unsafe {
            ptr::copy_nonoverlapping(ptr::with_exposed_provenance(address), out.as_mut_ptr(), len);
        }
        Ok(len)
    }

    fn open(&mut self) -> Result<usize, ReceiveError> {
        let len = self.delivery.len;
        if !matches!(self.state, State::Held | State::Open) {
            return Err(ReceiveError::Returning);
        }
        // Revalidate even after a prior Open because the raw Kernel Client is
        // also available to callers. Only this operation uses the address.
        let (address, span) = share_view::open(self.delivery.token).map_err(ReceiveError::Open)?;
        let address = address.get();
        self.state = State::Open;
        if len > span
            || len > isize::MAX as usize
            || address == 0
            || address.checked_add(len).is_none()
        {
            return Err(ReceiveError::InvalidMapping);
        }
        Ok(address)
    }

    /// Withdraw the mapping while retaining the payload authority for another Open.
    pub fn shut(&mut self) -> Result<(), ReceiveError> {
        match self.state {
            State::Held => return Ok(()),
            State::Open | State::Shutting => self.state = State::Shutting,
            _ => return Err(ReceiveError::Returning),
        }
        share_view::shut(self.delivery.token).map_err(ReceiveError::Shut)?;
        self.state = State::Held;
        Ok(())
    }

    /// Return the root, including any mapping that remains open.
    pub fn finish(mut self) -> Result<(), (Self, ReceiveError)> {
        match self.retire() {
            Ok(()) => Ok(()),
            Err(error) => Err((self, error)),
        }
    }

    pub(super) fn retire(&mut self) -> Result<(), ReceiveError> {
        if matches!(self.state, State::Finished) {
            return Ok(());
        }
        self.state = State::Returning;
        pend::return_(self.delivery.token).map_err(ReceiveError::Return)?;
        self.state = State::Finished;
        Ok(())
    }

    pub(super) fn returning(&self) -> bool {
        matches!(self.state, State::Returning)
    }
}

impl Drop for Payload {
    fn drop(&mut self) {
        let _ = self.retire();
    }
}
