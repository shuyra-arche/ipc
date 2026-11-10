use std::sync::atomic::AtomicU8;

use twine::view::share::{Fail, Read, Ring, Write};

#[test]
fn partial_progress_wrap_and_drain_on_close() {
    let mut storage = std::array::from_fn::<_, 4, _>(|_| AtomicU8::new(0));
    let mut ring = Ring::new(&mut storage).unwrap();
    let (mut reader, mut writer) = ring.split();

    let mut out = [0; 3];
    assert_eq!(reader.read(&mut out), Ok(Read::Pending));
    assert_eq!(writer.write(b"abcdef"), Ok(Write::Bytes(4)));
    assert_eq!(writer.write(b"z"), Ok(Write::Pending));
    assert_eq!(reader.read(&mut out), Ok(Read::Bytes(3)));
    assert_eq!(&out, b"abc");

    assert_eq!(writer.write(b"efg"), Ok(Write::Bytes(3)));
    writer.close();
    assert_eq!(writer.write(b"h"), Err(Fail::Closed));
    assert_eq!(reader.read(&mut out), Ok(Read::Bytes(3)));
    assert_eq!(&out, b"def");
    assert_eq!(reader.read(&mut out), Ok(Read::Bytes(1)));
    assert_eq!(out[0], b'g');
    assert_eq!(reader.read(&mut out), Ok(Read::Eof));
}

#[test]
fn dropping_the_reader_closes_its_end() {
    assert!(matches!(Ring::new(&mut []), Err(Fail::InvalidCapacity)));
    let mut storage = [AtomicU8::new(0)];
    let mut ring = Ring::new(&mut storage).unwrap();
    let (mut reader, mut writer) = ring.split();

    assert_eq!(reader.read(&mut []), Ok(Read::Bytes(0)));
    assert_eq!(writer.write(&[]), Ok(Write::Bytes(0)));
    drop(reader);
    assert_eq!(writer.write(b"x"), Err(Fail::BrokenPipe));
}

#[test]
fn concurrent_transfer_preserves_order_and_does_not_drop_the_final_write() {
    const LEN: usize = 1024 * 1024;
    let mut storage = std::array::from_fn::<_, 127, _>(|_| AtomicU8::new(0));
    let mut ring = Ring::new(&mut storage).unwrap();
    let (mut reader, mut writer) = ring.split();

    std::thread::scope(move |threads| {
        threads.spawn(move || {
            let mut written = 0;
            while written < LEN {
                let value = (written % 251) as u8;
                match writer.write(&[value]).unwrap() {
                    Write::Bytes(1) => written += 1,
                    Write::Pending => std::thread::yield_now(),
                    other => panic!("unexpected write: {other:?}"),
                }
            }
        });

        let mut out = [0; 53];
        let mut received = 0;
        loop {
            match reader.read(&mut out).unwrap() {
                Read::Bytes(n) => {
                    for &byte in &out[..n] {
                        assert_eq!(byte, (received % 251) as u8);
                        received += 1;
                    }
                }
                Read::Pending => std::thread::yield_now(),
                Read::Eof => break,
            }
        }
        assert_eq!(received, LEN);
    });
}
