# Provenance

## Source

- Canonical repository: <https://github.com/anreonyr/sqware.git>
- Commit: `7848438d3251824471a458ee304adc1574fe07fb`
- Public source branch: `codex/shuyra-ipc-time-boundary` (PR37)
- Local extraction branch: `codex/shuyra-ipc-extraction` (not published)
- Historical attribution for `crates/ipc` (from
  `git log --format='%an <%ae>' -- crates/ipc` at the frozen commit): 28 commits
  by `anreonyr` under two addresses, 24 as
  `anreonyr <anreonyr@gmail.com>` and 4 as
  `anreonyr <88045926+anreonyr@users.noreply.github.com>`
  (commits `275e0f92`, `4438a180`, `90a9150b`, `9729a4de`). The frozen tip
  `7848438d…` is itself authored by `anreonyr <anreonyr@gmail.com>`. Authorship
  is recorded from `git log`; it is not inferred from Cargo metadata, which
  declares no `authors`.
- No license text was added, removed, or inferred. The source commit provides no
  license file in `crates/ipc`.

The provider maps the 32 tracked files under `source/crates/ipc` to the same
relative paths, and also carries two owner locks that existed but were ignored
at the source. The source working tree was clean at the commit above before and
after this snapshot; no source file was changed.

## 32-file mapping

`source_sha256` is the hash at the source commit. Rows marked `same` have the
same hash in this repository. Rows marked `adapted` are explained below.

| source | provider | source_sha256 | state |
|---|---|---|---|
| `crates/ipc/Cargo.toml` | `Cargo.toml` | `a7ce417cf8674fee9c671f3486078d65808d5ae879531b9fd3ea37f36a5e592c` | adapted |
| `crates/ipc/src/debug.rs` | `src/debug.rs` | `f35f0a89c815da94a44522a830318951aa6c70be24790a9e935e1e1f7b33e7ab` | same |
| `crates/ipc/src/hand/mod.rs` | `src/hand/mod.rs` | `b753c801ba85d3d8e76b6f07e7f329752de7e5ac02afd7b14c624c4335a9616a` | same |
| `crates/ipc/src/hand/receiver.rs` | `src/hand/receiver.rs` | `7fc7424f22a9d6305519c3ad8cbabaff75884406517ced3124458c708d30f65d` | same |
| `crates/ipc/src/hand/sender.rs` | `src/hand/sender.rs` | `9dbae4d73698d3cae4226e2181b17732520e8d21d9f17a98910ad8fca009a989` | same |
| `crates/ipc/src/lib.rs` | `src/lib.rs` | `6f639e1f2cfef7d207cc4d14bb63f047e4f75fd2109890106d46473c1a4f86a7` | same |
| `crates/ipc/src/rack/bell.rs` | `src/rack/bell.rs` | `e6dcef1b2dcbf4c96e16bc0afe4e8e11e2ba727fa4d8ba674ba657b21f86ddf5` | same |
| `crates/ipc/src/rack/mod.rs` | `src/rack/mod.rs` | `2754c92eee08f68672fa463ff85744d2d5c91fa2ecadd89cdaf2f0ed508c92f8` | same |
| `crates/ipc/src/rack/reader.rs` | `src/rack/reader.rs` | `348888ce48c04952bf0fce64473e0b49cd1ed6d7787eb106e579d710443d4bfd` | same |
| `crates/ipc/src/rack/ring.rs` | `src/rack/ring.rs` | `002633fbd7dd9e565c2301b90e54c4eea862d1357e314d5b3255fada2d86a4bb` | same |
| `crates/ipc/src/rack/writer.rs` | `src/rack/writer.rs` | `b67c65c69dfad8b5428afa50265f513acb30408352a028af7856c5507636b2dc` | same |
| `crates/ipc/src/rpc.rs` | `src/rpc.rs` | `4e2653164f7dfab56a106f4600e5ac0d6905fbd8a3f7cd3150a2f118c81092b7` | same |
| `crates/ipc/src/session/establish.rs` | `src/session/establish.rs` | `909016614ce19e2cc62893d411fcafc89ea3c5cb6962d38c02fbebbfd73e12be` | same |
| `crates/ipc/src/session/exchange.rs` | `src/session/exchange.rs` | `4fc545a9bd3d85fec2c1481bbf6e10a0abca20831972815aeba2ac458420f37c` | same |
| `crates/ipc/src/session/mod.rs` | `src/session/mod.rs` | `889556289081d858b746c286725772ef9980e698b659a2a0375bf64e332de3a7` | same |
| `crates/ipc/src/session/state.rs` | `src/session/state.rs` | `c214b52bc951a03190b2b7e5168a73c20c0dc109a1356bbf21b61b47581d76dd` | same |
| `crates/ipc/src/time.rs` | `src/time.rs` | `2e52128d1f781a3d53fdec1b9c921a1a9c0741f2fc53d118e1af49aa26418ffe` | same |
| `crates/ipc/tests/host/.gitignore` | `tests/host/.gitignore` | `306fd52e74fca6746e12acc750f232de15e71da917756a1270d74c91f5eb7368` | same |
| `crates/ipc/tests/host/Cargo.lock` | `tests/host/Cargo.lock` | `1a5a697d2feff40e5fac91a453b40236f1e5dac787b988dcc81640723b4152fa` | same |
| `crates/ipc/tests/host/Cargo.toml` | `tests/host/Cargo.toml` | `dc65b1e2055afee98a9578518e59904c824d09e257ebd5d92f12f34d2840dcce` | same |
| `crates/ipc/tests/host/lib.rs` | `tests/host/lib.rs` | `70f9e6bc267bb6191d359572c920a4c1a6c3dd79b31bdbb9a3cdb752d4df96c0` | same |
| `crates/ipc/tests/host/time_boundary.rs` | `tests/host/time_boundary.rs` | `42c5fc990970368375021be5892cf17322e0ed7e2cd59925130cf4f8e52800cd` | same |
| `crates/ipc/tests/rack-reader-host/.gitignore` | `tests/rack-reader-host/.gitignore` | `306fd52e74fca6746e12acc750f232de15e71da917756a1270d74c91f5eb7368` | same |
| `crates/ipc/tests/rack-reader-host/Cargo.lock` | `tests/rack-reader-host/Cargo.lock` | `57df9a3274fc2d4c2c8465218d82d25228049ffa310ccc12001f7f2672f84c79` | same |
| `crates/ipc/tests/rack-reader-host/Cargo.toml` | `tests/rack-reader-host/Cargo.toml` | `7472534b0c4ab49c2b0efe9f692fb09d4d67ab3d4211fb4ce1d9e42fc7435eb6` | same |
| `crates/ipc/tests/rack-reader-host/lib.rs` | `tests/rack-reader-host/lib.rs` | `4f14abfa4bea6fdc8249591f076f04914fac3ddf29ba750a8ee4a68c9c455b03` | same |
| `crates/ipc/tests/session-establish/.gitignore` | `tests/session-establish/.gitignore` | `c1e953ee360e77de57f7b02f1b7880bd6a3dc22d1a69e953c2ac2c52cc52d247` | adapted |
| `crates/ipc/tests/session-establish/Cargo.toml` | `tests/session-establish/Cargo.toml` | `83883fd41d0a9dd6b38b21587b58de2365fcee27518c7cc6c1f71a4ebf3ad121` | same |
| `crates/ipc/tests/session-establish/lib.rs` | `tests/session-establish/lib.rs` | `e4c3370f150fba8d0c8240ea625cd91e6daad6ca04ec8c9663827310227bb15b` | same |
| `crates/ipc/tests/session-exchange/.gitignore` | `tests/session-exchange/.gitignore` | `07d64eb09a56c853b7853a47276799b38d7d6d25a4314b0c1abc10432c214e17` | adapted |
| `crates/ipc/tests/session-exchange/Cargo.toml` | `tests/session-exchange/Cargo.toml` | `a7c11c81bd3b1699796729cdbd15e8546e357684642a8ceaa87be8d53c3c04dc` | adapted |
| `crates/ipc/tests/session-exchange/lib.rs` | `tests/session-exchange/lib.rs` | `8a018f26009af6d45b5be3b18b7acd17fab920f4d65e5949fb73614c38217deb` | adapted |

The root `rust-toolchain.toml` and root `.gitignore` were copied from the same
source commit (`nightly-2026-09-04`); the latter keeps `**/target/` ignored.
The two ignored owner locks also come from that source working tree. The four
`tests/*` packages are std/host-only fixture workspaces, not production
dependencies of `ipc`. No production `.rs` file differs from source: the 16
hashes above were rechecked after extraction.

## Necessary adaptations

- **`Cargo.toml`**: the source `crates/ipc/Cargo.toml` used workspace
  inheritance. The provider makes `version = "0.1.0"` and `edition = "2024"`
  explicit and adds an independent `[workspace]` with `resolver = "3"`.
  `[lib] test = false`, `bench = false`, `doctest = false` are unchanged, as are
  the `env`/`wire`/`resource` canonical Git full revisions.
- **`tests/session-exchange/Cargo.toml`**: `system-api` was used only by the
  cross-owner `lookup` projection and was removed.
- **`tests/session-exchange/lib.rs`**: one public, non-`#[cfg(test)]` delegate
  was added to the fixture's existing `session::Session`:

  ```rust
  pub fn call<C: Contract>(&self, request: C::Request, within: crate::Wait)
      -> Result<<C::Response as crate::Message>::In, exchange::CallFail> {
      exchange::call::<C>(self, request, within)
  }
  ```

  This delegates to the real owned `exchange::call`; production visibility was
  not changed and no exchange/lookup implementation was copied. The same file
  removed the cross-owner
  `#[path = "../../../../programs/src/system/client/src/operator/lookup.rs"]`
  module and its two `lookup_regression` tests. Those two tests are not IPC
  tests and remain unchanged in source. The other 15 IPC test bodies are
  untouched.
- **`tests/session-exchange/.gitignore` and
  `tests/session-establish/.gitignore`**: only the `/Cargo.lock` line was
  removed; `/target` remains ignored. Both owner locks are therefore
  candidate-tracked.

## Locks

All five locks are candidate-tracked (no `.gitignore` rule covers them):

| lock | sha256 | provenance |
|---|---|---|
| `Cargo.lock` | `0e3cb6731619ae2afe00cd8daea87fd416ed552fa0d4b9532f5e9e6f179fca6f` | newly generated; not copied from the source root lock (`17083aba…`) |
| `tests/host/Cargo.lock` | `1a5a697d2feff40e5fac91a453b40236f1e5dac787b988dcc81640723b4152fa` | byte-identical to source |
| `tests/rack-reader-host/Cargo.lock` | `57df9a3274fc2d4c2c8465218d82d25228049ffa310ccc12001f7f2672f84c79` | byte-identical to source |
| `tests/session-establish/Cargo.lock` | `4b1b6c445823c75bd83736a21fbe54813ddb8e8959dd184418c55fa09e99818d` | byte-identical to the source ignored lock |
| `tests/session-exchange/Cargo.lock` | `45d95b5a5e69c752b5df56b8c0439ba3e7ceb7c302d39d84b078912ceaaa6954` | regenerated after dropping `system-api`; source ignored lock was `63ba1f25…` |

The exchange lock legally shrinks from 11 packages to 1 (the fixture itself).
It is **not** byte-identical to the source lock and is not claimed to be.
`unicode-ident` was not unified across the retained owner locks: host keeps
`1.0.24`, rack keeps `1.0.26`. The fresh root lock is neither a copy of the
source root lock nor an in-place upgrade of an existing pin: its initial
resolution selected `bitflags 2.13.2` and `unicode-ident 1.0.27`, while the
source root lock records `bitflags 2.13.1` and `unicode-ident 1.0.24`. This is
allowed for the new root workspace's initial resolution; the direct
`env`/`wire`/`resource` Git pins and the three retained owner locks were
unchanged.

## Test counts and remaining move

Provider: `28 + 15 + 23 + 3 = 69` tests. The source still contains the two
lookup tests, so the IPC-owned source total remains 71 before the consumer
change. The planned consumer move sends those two tests to
`programs/tests/operator-client` (`7 → 9`) using a real public fixture, making
the final split `69 + 87 = 156`, the same total as the original `71 + 85`.
This round does not perform that source change, and does not describe the
session-exchange suite as 17 tests.

## Runtime boundary

Native host clock remains unsupported. PR14's compile-gate history is preserved
in the source
`docs/migration-stage3-ipc-time-boundary.md`: the first exploratory Native host
clock probe exited 101, and the gate was narrowed to compile-only
signature/traits/inference coverage; no Native success stub was added. The
`with_clock` mechanism and the owned fixtures passing do not constitute Native
runtime execution. No QEMU was run in this round.

## Local verification

Sixteen commands were run under
`CARGO_TARGET_DIR=/home/anreonyr/Develop/shuyra/target/shared-build/pr15-provider`:
the eight commands listed in `README.md` with `--locked`, then the same eight with
`--locked --offline`. All returned exit 0. Host, session-exchange,
session-establish, and rack-reader-host reported `28/15/23/3` tests. The
session-exchange `--lib` host build confirms the public fixture does not depend
on `cfg(test)`. Lock hashes were identical before, after the locked matrix, and
after the offline matrix.

## Deferred hard gate

This is a local provider snapshot only. After the provider is published to a
public repository with a full commit SHA, the consumer must probe a real
canonical Git fixture dependency (not a local path), call the public
`Session::call` for a finite timeout and then `Forever` on the same session,
and verify it stays closed with `raw` pushes `1` and the endpoint sealed. That
probe has **not** run and is not claimed. If the nested fixture package cannot
be discovered or called through Git, the consumer must not switch to a local
backlink; the provider must publish a corrected public SHA.

No commit, push, new Git repository, source edit, consumer edit, or Stage 3/4
completion claim is made here.
