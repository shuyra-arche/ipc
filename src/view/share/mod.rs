//! SPSC byte rings. The backing storage and both roles share one address space.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

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

#[repr(align(64))]
struct Cursor {
    position: AtomicU64,
    closed: AtomicBool,
}

impl Cursor {
    const fn new() -> Self {
        Self {
            position: AtomicU64::new(0),
            closed: AtomicBool::new(false),
        }
    }

    fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
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
        if bytes.is_empty() || bytes.len() > MAX_CAPACITY {
            return Err(Fail::InvalidCapacity);
        }
        Ok(Self {
            read: Cursor::new(),
            write: Cursor::new(),
            bytes,
        })
    }

    pub fn split(&mut self) -> (Reader<'_>, Writer<'_>) {
        (Reader { ring: self }, Writer { ring: self })
    }

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
}

pub struct Reader<'a> {
    ring: &'a Ring<'a>,
}

impl Reader<'_> {
    pub fn read(&mut self, out: &mut [u8]) -> Result<Read, Fail> {
        let ring = self.ring;
        if ring.read.closed() {
            return Err(Fail::Closed);
        }
        if out.is_empty() {
            return Ok(Read::Bytes(0));
        }
        let read = ring.read.position.load(Ordering::Relaxed);
        // Load close first. Observing EOF must also observe the last write.
        let closed = ring.write.closed();
        let write = ring.write.position.load(Ordering::Acquire);
        let n = out.len().min(ring.available(read, write)?);
        if n == 0 {
            return Ok(if closed { Read::Eof } else { Read::Pending });
        }
        for (offset, byte) in out[..n].iter_mut().enumerate() {
            *byte = ring.at(read, offset).load(Ordering::Relaxed);
        }
        ring.read.position.store(read + n as u64, Ordering::Release);
        Ok(Read::Bytes(n))
    }

    pub fn close(&mut self) {
        self.ring.read.close();
    }
}

impl Drop for Reader<'_> {
    fn drop(&mut self) {
        self.close();
    }
}

pub struct Writer<'a> {
    ring: &'a Ring<'a>,
}

impl Writer<'_> {
    pub fn write(&mut self, input: &[u8]) -> Result<Write, Fail> {
        let ring = self.ring;
        if ring.write.closed() {
            return Err(Fail::Closed);
        }
        if ring.read.closed() {
            return Err(Fail::BrokenPipe);
        }
        if input.is_empty() {
            return Ok(Write::Bytes(0));
        }
        let write = ring.write.position.load(Ordering::Relaxed);
        let read = ring.read.position.load(Ordering::Acquire);
        let n = input
            .len()
            .min(ring.bytes.len() - ring.available(read, write)?);
        if n == 0 {
            return Ok(Write::Pending);
        }
        let next = write.checked_add(n as u64).ok_or(Fail::Exhausted)?;
        for (offset, &byte) in input[..n].iter().enumerate() {
            ring.at(write, offset).store(byte, Ordering::Relaxed);
        }
        ring.write.position.store(next, Ordering::Release);
        Ok(Write::Bytes(n))
    }

    pub fn close(&mut self) {
        self.ring.write.close();
    }
}

impl Drop for Writer<'_> {
    fn drop(&mut self) {
        self.close();
    }
}
