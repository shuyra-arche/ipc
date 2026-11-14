#![no_std]
#![no_main]

mod fixture;

use core::fmt::Write as _;
use kernel_api::{
    abi::call::sign::SignReply,
    model::{
        identity::PendId,
        modifiable::Modifiable,
        pend::{Operations, ShareRights},
        sign::{SignRights, SignSource},
        unit::{UnitQuery, UnitTarget},
        wait::Wait,
    },
};
use kernel_client::call::{clone_view, pend, room, sign, unit};
use twine::{
    sign::Wake,
    view::share::{MappedReader, MappedWriter, MappingError, Pages, Read, Write},
};

use fixture::{Console, Mode, deadline, exit, inspect, retry, ring};

const CAPACITY: usize = 4;

core::arch::global_asm!(
    ".section .text.entry",
    ".global _start",
    "_start:",
    "tail start",
);

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(Console, "FAIL twine-share {info}");
    exit(1)
}

#[unsafe(no_mangle)]
extern "C" fn start() -> ! {
    let license = fixture::license();
    assert_eq!(kernel_api::abi::VERSION, 11);
    clone_roundtrip(license);
    let code = fixture::Code::new();
    for mode in [Mode::Drain, Mode::Revoke, Mode::Exit] {
        run(license, &code, mode);
    }
    writeln!(
        Console,
        "PASS twine-share abi=11 clone=1 reply=1 teams=2 wrap=1 drain=1 revoke=1 exit=1"
    )
    .unwrap();
    exit(0)
}

fn run(license: PendId, code: &fixture::Code, mode: Mode) {
    let child = code.spawn(license, mode, reader);
    let (team, target) = (child.team, child.target);
    let UnitTarget::Task(task) = target else {
        unreachable!();
    };
    assert_ne!(inspect(UnitQuery::Caller).team, team);
    assert_eq!(inspect(UnitQuery::Target(target)).team, team);

    let slots = [(); 3].map(|()| fixture::allocate(license, 1));
    let [data, read, write] = slots.map(|slot| fixture::share(license, slot, Modifiable::Yes));
    let incoming = sign::create(license).unwrap();
    let outgoing = sign::create(license).unwrap();
    let rights = Operations::Share(ShareRights::ASSOCIATE);
    let data_grant = fixture::grant(data, Some(task), rights, Modifiable::No);
    fixture::grant(read, Some(task), rights, Modifiable::Yes);
    fixture::grant(write, Some(task), rights, Modifiable::No);
    fixture::grant(
        outgoing,
        Some(task),
        Operations::Sign(SignRights::LISTEN | SignRights::CONSUME | SignRights::ROUTE),
        Modifiable::No,
    );
    fixture::grant(
        incoming,
        Some(task),
        Operations::Sign(SignRights::TRIGGER),
        Modifiable::No,
    );
    let local_read = fixture::grant(read, None, rights, Modifiable::No);
    let join = SignSource::Join { target };
    sign::attach(incoming, join, 0).unwrap();
    let mut wake = Wake::new(incoming, outgoing);
    // SAFETY: The new RAM slots are zeroed, distinct and exclusively used by
    // this test. The child receives the complementary permissions.
    let mut writer = unsafe {
        MappedWriter::new(
            fixture::context(fixture::space()),
            Pages {
                data,
                read: local_read,
                write,
            },
            CAPACITY,
        )
        .unwrap()
    };
    assert_eq!(ring(|| writer.write(b"abcd")).unwrap(), Write::Bytes(4));
    assert_eq!(ring(|| writer.write(b"e")).unwrap(), Write::Pending);
    wake.notify().unwrap();
    wake.notify().unwrap();
    unit::resume(target).unwrap();
    triggered(&mut wake);

    let expected = match mode {
        Mode::Drain => {
            assert_eq!(ring(|| writer.write(b"efg")).unwrap(), Write::Bytes(3));
            ring(|| writer.close()).unwrap();
            wake.notify().unwrap();
            10
        }
        Mode::Revoke => {
            fixture::wait_until_blocked(target);
            retry(|| pend::revoke(task, data_grant));
            // Only the reader's branch is withdrawn. The parent can still
            // access its own data page and publish the remaining bytes.
            assert_eq!(ring(|| writer.write(b"efg")).unwrap(), Write::Bytes(3));
            11
        }
        Mode::Exit => 12,
    };
    assert_eq!(
        wake.wait(deadline()).unwrap(),
        SignReply::Source {
            source: join,
            fail: None
        },
    );
    assert_eq!(inspect(UnitQuery::Target(target)).result, Some(expected));
    if mode == Mode::Exit {
        // A peer exit is reported via Join even without a ring close flag.
        assert_eq!(ring(|| writer.write(b"efg")).unwrap(), Write::Bytes(3));
        assert_eq!(ring(|| writer.write(b"h")).unwrap(), Write::Pending);
    }

    ring(|| writer.finish()).unwrap();
    fixture::reap(&child);
    for token in [incoming, outgoing] {
        sign::delete(token).unwrap();
        retry(|| pend::return_(token));
    }
    for token in [local_read, data, read, write] {
        retry(|| pend::return_(token));
    }
    for slot in slots {
        fixture::delete_slot(slot);
    }
}

extern "C" fn reader(args: usize, count: usize) -> ! {
    assert_eq!(count, 1);
    let mode = unsafe { (args as *const usize).read() };
    let [space, data, read, write, incoming, outgoing] = fixture::collect();
    let mut wake = Wake::new(incoming, outgoing);
    // SAFETY: The parent exclusively allocates and shares these zeroed RAM
    // regions. It waits for this Task to block before revoking in Revoke mode.
    let mut reader = unsafe {
        MappedReader::new(
            fixture::context(space),
            Pages { data, read, write },
            CAPACITY,
        )
        .unwrap()
    };
    triggered(&mut wake);
    let mut out = [0; 4];
    assert_eq!(ring(|| reader.read(&mut out[..3])).unwrap(), Read::Bytes(3));
    assert_eq!(&out[..3], b"abc");

    if mode == Mode::Revoke as usize {
        let UnitTarget::Task(task) = inspect(UnitQuery::Caller).target else {
            unreachable!();
        };
        let source = SignSource::Inspect { task, token: data };
        sign::attach(incoming, source, 0).unwrap();
        wake.notify().unwrap();
        assert!(matches!(
            wake.wait(deadline()).unwrap(),
            SignReply::Source { source: found, fail: Some(_) } if found == source
        ));
        assert!(matches!(
            ring(|| reader.read(&mut out)),
            Err(MappingError::Access { token, .. }) if token == data
        ));
        exit(11)
    }

    wake.notify().unwrap();
    if mode == Mode::Exit as usize {
        // Give the writer time to sleep on Join. There is no close or wake on
        // exit; the Kernel must report the lifecycle event.
        assert_eq!(
            wake.wait(Wait::At(room::clock().saturating_add(100_000_000)))
                .unwrap(),
            SignReply::Pending,
        );
        exit(12)
    }

    assert_eq!(mode, Mode::Drain as usize);
    triggered(&mut wake);
    assert_eq!(ring(|| reader.read(&mut out)).unwrap(), Read::Bytes(4));
    assert_eq!(&out, b"defg");
    assert_eq!(ring(|| reader.read(&mut out)).unwrap(), Read::Eof);
    ring(|| reader.close()).unwrap();
    exit(10)
}

fn triggered(wake: &mut Wake) {
    let reply = wake.wait(deadline()).unwrap();
    assert!(matches!(reply, SignReply::Triggered { .. }), "{reply:?}");
}

fn clone_roundtrip(license: PendId) {
    use kernel_api::model::{clone_view::CloneLimits, identity::Mark};
    use twine::view::clone::{Receiver, Reply, Sender};
    use weave::{FrameReceiver, FrameSender};
    let mut memory = fixture::context(fixture::space());
    let queue = clone_view::create(
        license,
        Mark::NONE,
        CloneLimits {
            max_len: 5000,
            max_messages: 2,
            max_bytes: 10000,
        },
    )
    .unwrap();
    let mut sender = Sender::new(queue);
    let bytes = [0x5a; 5000];
    sender.send(&bytes).unwrap();
    let mut receiver = Receiver::new(queue, &mut memory);
    assert_eq!(receiver.peek().unwrap().0, bytes.len());
    let mut payload = receiver.pull(bytes.len()).unwrap();
    // A delivery survives the originating queue's closure.
    clone_view::delete(queue).unwrap();
    let mut out = [0; 5000];
    assert_eq!(payload.copy(&mut out, receiver.memory()).unwrap(), 5000);
    assert_eq!(out, bytes);
    payload.shut(receiver.memory()).unwrap();
    assert_eq!(payload.copy(&mut out, receiver.memory()).unwrap(), 5000);
    payload.finish(receiver.memory()).unwrap();
    drop(receiver);
    retry(|| pend::return_(queue));

    let UnitTarget::Task(caller) = inspect(UnitQuery::Caller).target else {
        panic!("caller");
    };
    let mut reply = Reply::create(license, 32, memory).unwrap();
    let token = reply.grant(caller).unwrap();
    let mut sender = Sender::reply(token, caller).unwrap();
    sender.send(b"native-reply").unwrap();
    let mut out = [0; 32];
    let frame = reply.receive(&mut out).unwrap();
    assert_eq!(&out[..frame.len], b"native-reply");
    assert_eq!(frame.metadata, caller);
    assert!(reply.receive(&mut out).is_err());
    reply.finish().unwrap();
}
