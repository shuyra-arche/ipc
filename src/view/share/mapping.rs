//! Task-local ring endpoints with independently authorized native publications.

use super::{Cursor, Fail, Parts, Read, Write, validate_capacity};
use crate::view::memory::{Mapping, MappingError as MemoryError, Memory};
use core::{ptr, slice, sync::atomic::AtomicU8};
use kernel_api::model::identity::PendId;

/// Share source Pends, each with ASSOCIATE and the required write ceiling.
#[derive(Clone, Copy, Debug)]
pub struct Pages {
    pub data: PendId,
    pub read: PendId,
    pub write: PendId,
}

#[derive(Debug)]
pub enum MappingError {
    Ring(Fail),
    Access { token: PendId, error: MemoryError },
    InvalidMapping,
}

impl MappingError {
    /// A rejected Busy or an accepted Pending can be retried with this endpoint.
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Access { error, .. } if error.is_busy())
    }
}

struct Mapped<M: Memory> {
    memory: M,
    pages: Pages,
    data: Mapping,
    read: Mapping,
    write: Mapping,
    capacity: usize,
}

impl<M: Memory> Mapped<M> {
    fn new(memory: M, pages: Pages, capacity: usize, writer: bool) -> Result<Self, MappingError> {
        validate_capacity(capacity).map_err(MappingError::Ring)?;
        if pages.data == pages.read || pages.data == pages.write || pages.read == pages.write {
            return Err(MappingError::InvalidMapping);
        }
        let create = |token, bytes, writable| {
            Mapping::new(token, bytes, writable)
                .map_err(|error| MappingError::Access { token, error })
        };
        Ok(Self {
            memory,
            pages,
            capacity,
            data: create(pages.data, capacity, writer)?,
            read: create(pages.read, size_of::<Cursor>(), !writer)?,
            write: create(pages.write, size_of::<Cursor>(), writer)?,
        })
    }

    fn parts(&mut self) -> Result<Parts<'_>, MappingError> {
        let data = address::<AtomicU8>(&mut self.data, &mut self.memory, self.pages.data)?;
        let read = address::<Cursor>(&mut self.read, &mut self.memory, self.pages.read)?;
        let write = address::<Cursor>(&mut self.write, &mut self.memory, self.pages.write)?;
        // SAFETY: Constructor contracts cover layout, exclusivity and lifetime.
        // All Kernel calls finish before the references are constructed.
        Ok(unsafe {
            Parts {
                bytes: slice::from_raw_parts(ptr::with_exposed_provenance(data), self.capacity),
                read: &*ptr::with_exposed_provenance(read),
                write: &*ptr::with_exposed_provenance(write),
            }
        })
    }

    fn close(&mut self, writer: bool) -> Result<(), MappingError> {
        let (mapping, token) = if writer {
            (&mut self.write, self.pages.write)
        } else {
            (&mut self.read, self.pages.read)
        };
        let cursor = address::<Cursor>(mapping, &mut self.memory, token)?;
        // SAFETY: Only the role's writable cursor is accessed, with the same
        // layout and mapping lifetime as parts().
        unsafe { (&*ptr::with_exposed_provenance::<Cursor>(cursor)).close() };
        Ok(())
    }

    fn finish(&mut self) -> Result<(), MappingError> {
        for (mapping, token) in [
            (&mut self.data, self.pages.data),
            (&mut self.read, self.pages.read),
            (&mut self.write, self.pages.write),
        ] {
            mapping
                .finish(&mut self.memory)
                .map_err(|error| MappingError::Access { token, error })?;
        }
        Ok(())
    }
}

impl<M: Memory> Drop for Mapped<M> {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

fn address<T>(
    mapping: &mut Mapping,
    memory: &mut impl Memory,
    token: PendId,
) -> Result<usize, MappingError> {
    let address = mapping
        .address(memory)
        .map_err(|error| MappingError::Access { token, error })?;
    if !address.is_multiple_of(align_of::<T>()) {
        return Err(MappingError::InvalidMapping);
    }
    Ok(address)
}

/// The single consumer of a ring backed by Kernel Share sources.
pub struct MappedReader<M: Memory>(Mapped<M>);

impl<M: Memory> MappedReader<M> {
    /// Binds Share sources and owns their separate CPU reservations.
    ///
    /// # Safety
    ///
    /// Both endpoints must agree on capacity and three disjoint base-Cell RAM regions,
    /// exclusively reserved for this ring. Data holds initialized atomic bytes.
    /// Each cursor starts at offset zero of a 64-byte-aligned region, with two
    /// zeroed u64 atomics (position and close flag). There is exactly one reader
    /// and writer. The reader can write its cursor and read the other regions.
    /// All accesses follow this layout and atomic protocol. Coordinate source
    /// withdrawal, physical mutation and retirement at quiescent boundaries;
    /// publications must remain valid until each read or close has finished.
    pub unsafe fn new(memory: M, pages: Pages, capacity: usize) -> Result<Self, MappingError> {
        Mapped::new(memory, pages, capacity, false).map(Self)
    }

    pub fn read(&mut self, out: &mut [u8]) -> Result<Read, MappingError> {
        self.0.parts()?.read(out).map_err(MappingError::Ring)
    }

    /// Publish the close flag before notifying the writer.
    pub fn close(&mut self) -> Result<(), MappingError> {
        self.0.close(false)
    }

    /// Retire CPU reservations without returning the borrowed source Pends.
    /// Call close first if the peer should observe a graceful shutdown.
    pub fn finish(&mut self) -> Result<(), MappingError> {
        self.0.finish()
    }
}

/// The single producer of a ring backed by Kernel Share sources.
pub struct MappedWriter<M: Memory>(Mapped<M>);

impl<M: Memory> MappedWriter<M> {
    /// Binds Share sources and owns their separate CPU reservations.
    ///
    /// # Safety
    ///
    /// The reader constructor's layout, exclusivity and lifetime requirements
    /// apply. The writer needs write access to data and its own cursor, and read
    /// access to the reader cursor.
    pub unsafe fn new(memory: M, pages: Pages, capacity: usize) -> Result<Self, MappingError> {
        Mapped::new(memory, pages, capacity, true).map(Self)
    }

    pub fn write(&mut self, input: &[u8]) -> Result<Write, MappingError> {
        self.0.parts()?.write(input).map_err(MappingError::Ring)
    }

    pub fn close(&mut self) -> Result<(), MappingError> {
        self.0.close(true)
    }

    pub fn finish(&mut self) -> Result<(), MappingError> {
        self.0.finish()
    }
}
