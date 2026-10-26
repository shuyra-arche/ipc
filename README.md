# ipc

IPC provider snapshot extracted from
[`anreonyr/sqware`](https://github.com/anreonyr/sqware.git) at commit
`7848438d3251824471a458ee304adc1574fe07fb`
(`codex/shuyra-ipc-time-boundary`, PR37). This repository is a projection of
`crates/ipc`, not a reimplementation.

## Layout

- `src/` — 16 production Rust files, byte-identical to the source commit.
- `tests/host` — 28 tests, including eight `time_boundary` tests.
- `tests/session-exchange` — 15 IPC tests; the fixture exposes a public
  `Session::call` delegate so a consumer can exercise the real exchange.
- `tests/session-establish` — 23 tests.
- `tests/rack-reader-host` — 3 tests.
- `Cargo.toml` — root `ipc` package and its own workspace (`resolver = "3"`).
- `Cargo.lock` plus the four owner locks — all candidate-tracked.

`env`, `wire`, and `resource` remain pinned to the source full revisions
(`d3000253…`, `0ff77dc4…`, `dbc96834…`); the direct Git pins are unchanged.
The root lock is a fresh initial resolution for this smaller workspace, not a
copy of the source root lock: it selected `bitflags 2.13.2` and
`unicode-ident 1.0.27`, where the source root lock records `bitflags 2.13.1`
and `unicode-ident 1.0.24`. The three retained owner locks are byte-identical
to source; the exchange lock shrank from 11 packages to 1 after the
lookup-only `system-api` closure was removed.

## Verification

```sh
cargo build --lib --locked
cargo build --lib --locked --target riscv64gc-unknown-none-elf
cargo build --lib --locked --release --target riscv64gc-unknown-none-elf
cargo test --locked --manifest-path tests/host/Cargo.toml --target x86_64-unknown-linux-gnu
cargo test --locked --manifest-path tests/session-exchange/Cargo.toml --target x86_64-unknown-linux-gnu
cargo test --locked --manifest-path tests/session-establish/Cargo.toml --target x86_64-unknown-linux-gnu
cargo test --locked --manifest-path tests/rack-reader-host/Cargo.toml --target x86_64-unknown-linux-gnu
cargo build --lib --locked --manifest-path tests/session-exchange/Cargo.toml --target x86_64-unknown-linux-gnu
```

The same eight commands were repeated with `--locked --offline` after the cache
was prepared. See `PROVENANCE.md` for source mapping, lock decisions, test
counts, and the boundaries that remain unverified.
