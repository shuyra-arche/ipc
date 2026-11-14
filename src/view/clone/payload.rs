//! Independent read-only deliveries and native CPU reservation retirement.

use super::ReceiveError;
use crate::view::memory::{Mapping, Memory};
use core::{marker::PhantomData, ptr};
use kernel_api::model::{clone_view::PullOutcome, identity::TaskId};
use kernel_client::call::pend;

#[derive(Debug)]
enum State {
    Held,
    Returning,
    Finished,
}

/// One successful Clone Pull. Finish explicitly to observe cleanup failures.
#[derive(Debug)]
#[must_use = "finish the payload to observe and retry cleanup failures"]
pub struct Payload {
    delivery: PullOutcome,
    mapping: Option<Mapping>,
    state: State,
    task: PhantomData<*mut ()>,
}

impl Payload {
    pub(super) fn new(delivery: PullOutcome) -> Self {
        Self {
            delivery,
            mapping: None,
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

    /// Copy a complete immutable delivery through a private read-only Nest
    /// reservation. Reuse the same Task context for all calls on this payload.
    /// No caller code or Kernel call runs while copying the bytes.
    pub fn copy(
        &mut self,
        out: &mut [u8],
        memory: &mut impl Memory,
    ) -> Result<usize, ReceiveError> {
        let len = self.len();
        if out.len() < len {
            return Err(ReceiveError::Capacity { required: len });
        }
        if !matches!(self.state, State::Held) {
            return Err(ReceiveError::Returning);
        }
        if self.mapping.is_none() {
            self.mapping =
                Some(Mapping::new(self.delivery.token, len, false).map_err(ReceiveError::Mapping)?);
        }
        let address = self
            .mapping
            .as_mut()
            .unwrap()
            .address(memory)
            .map_err(ReceiveError::Mapping)?;
        // SAFETY: Pull owns immutable copied Cells under a Task-local root.
        // This private reservation maps all of len read-only, with no caller
        // code or Kernel calls between validation and copying. The output is
        // a writable Rust buffer and cannot alias the copied payload Cells.
        unsafe {
            ptr::copy_nonoverlapping(ptr::with_exposed_provenance(address), out.as_mut_ptr(), len);
        }
        Ok(len)
    }

    /// Release this payload's CPU reservation while keeping its source Pend.
    pub fn shut(&mut self, memory: &mut impl Memory) -> Result<(), ReceiveError> {
        if !matches!(self.state, State::Held) {
            return Err(ReceiveError::Returning);
        }
        if let Some(mapping) = &mut self.mapping {
            mapping.finish(memory).map_err(ReceiveError::Mapping)?;
            self.mapping = None;
        }
        Ok(())
    }

    /// Settle publication/retirement before returning the independent root.
    pub fn finish(&mut self, memory: &mut impl Memory) -> Result<(), ReceiveError> {
        self.retire(memory)
    }

    pub(super) fn retire(&mut self, memory: &mut impl Memory) -> Result<(), ReceiveError> {
        if matches!(self.state, State::Finished) {
            return Ok(());
        }
        self.state = State::Returning;
        if let Some(mapping) = &mut self.mapping {
            mapping.finish(memory).map_err(ReceiveError::Mapping)?;
            self.mapping = None;
        }
        memory
            .return_pend(self.delivery.token)
            .map_err(ReceiveError::Return)?;
        self.state = State::Finished;
        Ok(())
    }

    pub(super) fn returning(&self) -> bool {
        matches!(self.state, State::Returning)
    }
}

impl Drop for Payload {
    fn drop(&mut self) {
        if !matches!(self.state, State::Finished) {
            // Best effort only. No context is available to settle a retained
            // native request. Explicit finish is required to release its address
            // reservation; the Kernel retains unsettled physical obligations.
            let _ = pend::return_(self.delivery.token);
        }
    }
}
