//! Coalesced notification between two endpoints.

use erra::Error;
use kernel_api::{
    abi::call::sign::{SignFail, SignReply},
    model::{identity::PendId, wait::Wait},
};
use kernel_client::call::sign;

/// A dedicated incoming Sign and the peer's Trigger grant.
pub struct Wake {
    incoming: PendId,
    outgoing: PendId,
}

impl Wake {
    /// The caller owns both Pends and any routes attached to the incoming Sign.
    pub fn new(incoming: PendId, outgoing: PendId) -> Self {
        Self { incoming, outgoing }
    }

    /// Call after publishing progress or closing the local endpoint.
    pub fn notify(&self) -> Result<(), Error<SignFail>> {
        super::trigger(self.outgoing)
    }

    /// Consume a direct notification before returning it. Retry the actual
    /// operation after a direct notification, and handle any routed event
    /// before touching storage. This must be the only direct consumer.
    pub fn wait(&mut self, wait: Wait) -> Result<SignReply, Error<SignFail>> {
        let reply = sign::listen(self.incoming, wait)?;
        if let SignReply::Triggered { sequence } = reply {
            sign::consume(self.incoming, sequence)?;
        }
        Ok(reply)
    }
}
