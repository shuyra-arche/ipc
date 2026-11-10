//! Task-local endpoints over three independently authorized Share regions.

use core::{marker::PhantomData, ptr, slice, sync::atomic::AtomicU8};

use erra::Error;
use kernel_api::{abi::call::share_view::ShareFail, model::identity::PendId};
use kernel_client::call::share_view;

use super::{Cursor, Fail, Parts, Read, Write, validate_capacity};

/// The current Task's grants for the ring's data and two cursor regions.
#[derive(Clone, Copy, Debug)]
pub struct Pages {
    pub data: PendId,
    pub read: PendId,
    pub write: PendId,
}

#[derive(Debug)]
pub enum MappingError {
    Ring(Fail),
    Open {
        token: PendId,
        error: Error<ShareFail>,
    },
    InvalidMapping,
}

impl MappingError {
    /// Open was contended. The operation can be retried without losing progress.
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Open { error, .. } if error.source.is_busy())
    }
}

struct Mapped {
    pages: Pages,
    capacity: usize,
    task: PhantomData<*mut ()>,
}

impl Mapped {
    fn new(pages: Pages, capacity: usize) -> Result<Self, MappingError> {
        validate_capacity(capacity).map_err(MappingError::Ring)?;
        if pages.data == pages.read || pages.data == pages.write || pages.read == pages.write {
            return Err(MappingError::InvalidMapping);
        }
        Ok(Self {
            pages,
            capacity,
            task: PhantomData,
        })
    }

    fn parts(&self) -> Result<Parts<'_>, MappingError> {
        let data = address::<AtomicU8>(self.pages.data, self.capacity)?;
        let read = address::<Cursor>(self.pages.read, size_of::<Cursor>())?;
        let write = address::<Cursor>(self.pages.write, size_of::<Cursor>())?;
        // SAFETY: The constructor contract covers the backing layout and
        // concurrent access. All Open calls finish before references exist.
        Ok(unsafe {
            Parts {
                bytes: slice::from_raw_parts(ptr::with_exposed_provenance(data), self.capacity),
                read: &*ptr::with_exposed_provenance(read),
                write: &*ptr::with_exposed_provenance(write),
            }
        })
    }

    fn close(&self, token: PendId) -> Result<(), MappingError> {
        let cursor = address::<Cursor>(token, size_of::<Cursor>())?;
        // SAFETY: Only this role's writable cursor is accessed, under the
        // same mapping and atomic-access contract as parts().
        unsafe { (&*ptr::with_exposed_provenance::<Cursor>(cursor)).close() };
        Ok(())
    }
}

fn address<T>(token: PendId, bytes: usize) -> Result<usize, MappingError> {
    let (address, span) =
        share_view::open(token).map_err(|error| MappingError::Open { token, error })?;
    let address = address.get();
    if span < bytes
        || address == 0
        || !address.is_multiple_of(align_of::<T>())
        || address.checked_add(bytes).is_none()
    {
        return Err(MappingError::InvalidMapping);
    }
    Ok(address)
}

/// The single consumer of a ring backed by Kernel Share Views.
pub struct MappedReader(Mapped);

impl MappedReader {
    /// Binds Task-local Share Pends. The caller manages their retirement.
    ///
    /// # Safety
    ///
    /// Both endpoints must agree on capacity and the same three disjoint RAM
    /// regions, reserved exclusively for this ring. The data holds initialized
    /// atomic bytes. Each cursor starts at offset zero of its 64-byte-aligned
    /// region, with two zeroed u64 atomics (position and close flag).
    /// All access follows this layout and the ring's atomic protocol.
    /// There must be only one reader and one writer, each with write access
    /// to its own cursor. The reader needs read access to data and the write
    /// cursor. Successful Open mappings must remain valid until the current
    /// read or close finishes. Coordinate withdrawal at a quiescent boundary.
    pub unsafe fn new(pages: Pages, capacity: usize) -> Result<Self, MappingError> {
        Mapped::new(pages, capacity).map(Self)
    }

    pub fn read(&mut self, out: &mut [u8]) -> Result<Read, MappingError> {
        self.0.parts()?.read(out).map_err(MappingError::Ring)
    }

    /// Close explicitly before notifying the writer. Drop touches no mapping.
    pub fn close(&mut self) -> Result<(), MappingError> {
        self.0.close(self.0.pages.read)
    }
}

/// The single producer of a ring backed by Kernel Share Views.
pub struct MappedWriter(Mapped);

impl MappedWriter {
    /// Binds Task-local Share Pends. The caller manages their retirement.
    ///
    /// # Safety
    ///
    /// The same requirements as the reader constructor apply to layout,
    /// exclusivity, atomic access and mapping lifetime. The writer needs
    /// write access to data and the write cursor, and read access to the
    /// read cursor.
    pub unsafe fn new(pages: Pages, capacity: usize) -> Result<Self, MappingError> {
        Mapped::new(pages, capacity).map(Self)
    }

    pub fn write(&mut self, input: &[u8]) -> Result<Write, MappingError> {
        self.0.parts()?.write(input).map_err(MappingError::Ring)
    }

    /// Close explicitly before notifying the reader. Drop touches no mapping.
    pub fn close(&mut self) -> Result<(), MappingError> {
        self.0.close(self.0.pages.write)
    }
}
