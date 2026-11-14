//! Task identity, shared request allocation and native call transport.
use core::{cell::RefCell, marker::PhantomData};
use erra::Error;
use kernel_api::{
    abi::{
        call::pend::PendFail,
        memory::{Call, RequestId},
    },
    address::VirtAddr,
    model::{identity::PendId, nest::NestId, pend::PendInfo},
};
use kernel_client::{
    call::pend,
    memory::{self, Response},
};

/// Supplies a Task's one shared request sequence and native memory transport.
/// Use `&RefCell<M>` to share a context between endpoints in the same Task.
///
/// # Safety
/// Replies and Pend observations must describe actual Kernel effects in the
/// current Task (or faithful emulation over owned storage). The space must be
/// the caller's Internal Nest. A successful Publish must establish the returned
/// CPU mapping; fabricated successes could make safe payload copies dereference
/// invalid memory. Invocations must preserve Client correlation checks.
pub unsafe trait Memory {
    fn space(&self) -> (PendId, NestId);
    fn next_request(&mut self) -> Option<RequestId>;
    fn invoke(&mut self, call: Call) -> Result<Response, memory::Error>;
    fn inspect_pend(&mut self, token: PendId) -> Result<PendInfo, Error<PendFail>>;
    fn return_pend(&mut self, token: PendId) -> Result<(), Error<PendFail>>;
}

// SAFETY: Delegates the complete contract without changing any result.
unsafe impl<M: Memory + ?Sized> Memory for &mut M {
    fn space(&self) -> (PendId, NestId) {
        (**self).space()
    }
    fn next_request(&mut self) -> Option<RequestId> {
        (**self).next_request()
    }
    fn invoke(&mut self, call: Call) -> Result<Response, memory::Error> {
        (**self).invoke(call)
    }
    fn inspect_pend(&mut self, token: PendId) -> Result<PendInfo, Error<PendFail>> {
        (**self).inspect_pend(token)
    }
    fn return_pend(&mut self, token: PendId) -> Result<(), Error<PendFail>> {
        (**self).return_pend(token)
    }
}

// SAFETY: RefCell serializes access to the original implementation.
unsafe impl<M: Memory> Memory for &RefCell<M> {
    fn space(&self) -> (PendId, NestId) {
        self.borrow().space()
    }
    fn next_request(&mut self) -> Option<RequestId> {
        self.borrow_mut().next_request()
    }
    fn invoke(&mut self, call: Call) -> Result<Response, memory::Error> {
        self.borrow_mut().invoke(call)
    }
    fn inspect_pend(&mut self, token: PendId) -> Result<PendInfo, Error<PendFail>> {
        self.borrow_mut().inspect_pend(token)
    }
    fn return_pend(&mut self, token: PendId) -> Result<(), Error<PendFail>> {
        self.borrow_mut().return_pend(token)
    }
}

/// Native Client transport. The allocator must share the Task's monotonically
/// increasing RequestId sequence with all other native memory users. Neither
/// dropping this context nor a failed delivery resets that sequence.
pub struct Native<R> {
    token: PendId,
    nest: NestId,
    requests: R,
    task: PhantomData<*mut ()>,
}

impl<R> Native<R> {
    /// # Safety
    ///
    /// `token` must authorize Reserve in the current Task's Internal Nest,
    /// and this context must only be used in that Task. A foreign Nest can
    /// successfully publish an address that is not mapped in the caller.
    pub unsafe fn new(token: PendId, nest: NestId, requests: R) -> Self {
        Self {
            token,
            nest,
            requests,
            task: PhantomData,
        }
    }
}

// SAFETY: All effects and observations come directly from the Kernel Client.
unsafe impl<R: FnMut() -> Option<RequestId>> Memory for Native<R> {
    fn space(&self) -> (PendId, NestId) {
        (self.token, self.nest)
    }
    fn next_request(&mut self) -> Option<RequestId> {
        (self.requests)()
    }
    fn invoke(&mut self, call: Call) -> Result<Response, memory::Error> {
        match call {
            Call::Flat(call) => memory::flat::invoke(call),
            Call::Nest(call) => memory::nest::invoke(call),
        }
    }
    fn inspect_pend(&mut self, token: PendId) -> Result<PendInfo, Error<PendFail>> {
        let mut bytes = [0; PendInfo::SIZE];
        pend::inspect(token, VirtAddr::new(bytes.as_mut_ptr().addr()))?;
        Ok(PendInfo::from_bytes(&bytes).expect("Kernel PendInfo"))
    }
    fn return_pend(&mut self, token: PendId) -> Result<(), Error<PendFail>> {
        pend::return_(token)
    }
}
