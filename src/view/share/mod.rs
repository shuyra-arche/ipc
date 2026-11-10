//! SPSC byte rings over local storage or Kernel Share mappings.

mod mapping;

use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};

pub use mapping::{MappedReader, MappedWriter, MappingError, Pages};

pub const MAX_CAPACITY: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fail {
    InvalidCapacity,
    Corrupt,
    Exhausted,
    Closed,
    BrokenPipe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read {
    Bytes(usize),
    Pending,
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Write {
    Bytes(usize),
    Pending,
}

#[repr(C, align(64))]
struct Cursor {
    position: AtomicU64,
    closed: AtomicU64,
}

impl Cursor {
    const fn new() -> Self {
        Self {
            position: AtomicU64::new(0),
            closed: AtomicU64::new(0),
        }
    }

    fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire) != 0
    }

    fn close(&self) {
        self.closed.store(1, Ordering::Release);
    }
}

/// Owns the cursors and exclusively borrows the byte storage. Splitting
/// requires an exclusive borrow, so only one pair of roles can be live.
pub struct Ring<'a> {
    read: Cursor,
    write: Cursor,
    bytes: &'a [AtomicU8],
}

impl<'a> Ring<'a> {
    pub fn new(bytes: &'a mut [AtomicU8]) -> Result<Self, Fail> {
        validate_capacity(bytes.len())?;
        Ok(Self {
            read: Cursor::new(),
            write: Cursor::new(),
            bytes,
        })
    }

    pub fn split(&mut self) -> (Reader<'_>, Writer<'_>) {
        let parts = Parts {
            read: &self.read,
            write: &self.write,
            bytes: self.bytes,
        };
        (Reader { parts }, Writer { parts })
    }
}

fn validate_capacity(capacity: usize) -> Result<(), Fail> {
    if !(1..=MAX_CAPACITY).contains(&capacity) {
        return Err(Fail::InvalidCapacity);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Parts<'a> {
    read: &'a Cursor,
    write: &'a Cursor,
    bytes: &'a [AtomicU8],
}

impl Parts<'_> {
    fn available(&self, read: u64, write: u64) -> Result<usize, Fail> {
        write
            .checked_sub(read)
            .filter(|&used| used <= self.bytes.len() as u64)
            .map(|used| used as usize)
            .ok_or(Fail::Corrupt)
    }

    fn at(&self, cursor: u64, offset: usize) -> &AtomicU8 {
        let capacity = self.bytes.len();
        let start = (cursor % capacity as u64) as usize;
        &self.bytes[(start + offset) % capacity]
    }

    fn read(&self, out: &mut [u8]) -> Result<Read, Fail> {
        if self.read.closed() {
            return Err(Fail::Closed);
        }
        if out.is_empty() {
            return Ok(Read::Bytes(0));
        }
        let read = self.read.position.load(Ordering::Relaxed);
        // Load close first. Observing EOF must also observe the last write.
        let closed = self.write.closed();
        let write = self.write.position.load(Ordering::Acquire);
        let n = out.len().min(self.available(read, write)?);
        if n == 0 {
            return Ok(if closed { Read::Eof } else { Read::Pending });
        }
        for (offset, byte) in out[..n].iter_mut().enumerate() {
            *byte = self.at(read, offset).load(Ordering::Relaxed);
        }
        self.read.position.store(read + n as u64, Ordering::Release);
        Ok(Read::Bytes(n))
    }

    fn write(&self, input: &[u8]) -> Result<Write, Fail> {
        if self.write.closed() {
            return Err(Fail::Closed);
        }
        if self.read.closed() {
            return Err(Fail::BrokenPipe);
        }
        if input.is_empty() {
            return Ok(Write::Bytes(0));
        }
        let write = self.write.position.load(Ordering::Relaxed);
        let read = self.read.position.load(Ordering::Acquire);
        let n = input
            .len()
            .min(self.bytes.len() - self.available(read, write)?);
        if n == 0 {
            return Ok(Write::Pending);
        }
        let next = write.checked_add(n as u64).ok_or(Fail::Exhausted)?;
        for (offset, &byte) in input[..n].iter().enumerate() {
            self.at(write, offset).store(byte, Ordering::Relaxed);
        }
        self.write.position.store(next, Ordering::Release);
        Ok(Write::Bytes(n))
    }
}

pub struct Reader<'a> {
    parts: Parts<'a>,
}

impl Reader<'_> {
    pub fn read(&mut self, out: &mut [u8]) -> Result<Read, Fail> {
        self.parts.read(out)
    }

    pub fn close(&mut self) {
        self.parts.read.close();
    }
}

impl Drop for Reader<'_> {
    fn drop(&mut self) {
        self.close();
    }
}

pub struct Writer<'a> {
    parts: Parts<'a>,
}

impl Writer<'_> {
    pub fn write(&mut self, input: &[u8]) -> Result<Write, Fail> {
        self.parts.write(input)
    }

    pub fn close(&mut self) {
        self.parts.write.close();
    }
}

impl Drop for Writer<'_> {
    fn drop(&mut self) {
        self.close();
    }
}
