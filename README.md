# ipc

Shuyra 的 `no_std` IPC 机制库，提供会话建立与交换、请求回复、共享环与时间边界。

- `src/session/`：会话状态、建立和交换。
- `src/hand/`、`src/rpc.rs`：收发与 RPC。
- `src/rack/`：共享环、读写端与门铃。
- `src/time.rs`：可注入 Clock、截止时间与剩余时间。
- `tests/host`、`tests/session-establish`、`tests/session-exchange`、
  `tests/rack-reader-host`：独立宿主测试工作区。

Native 时钟不支持宿主运行；测试使用注入时钟，Native 宿主检查仅验证签名与类型。
会话超时后关闭的行为由交换测试验证，不能以宿主 fake 后端替代原生资源生命周期验证。

## 工具链与维护

使用 `rust-toolchain.toml` 固定的 `nightly-2026-09-04`，首次运行需要 Rustup
下载工具链及 RV64 目标。依赖版本以 Cargo 清单和锁文件为准。
下列命令从仓库根目录执行；宿主测试不代表真实内核运行验证。

```sh
cargo build --lib --locked --target riscv64gc-unknown-none-elf
cargo test --locked --manifest-path tests/host/Cargo.toml --target x86_64-unknown-linux-gnu
cargo test --locked --manifest-path tests/session-exchange/Cargo.toml --target x86_64-unknown-linux-gnu
cargo test --locked --manifest-path tests/session-establish/Cargo.toml --target x86_64-unknown-linux-gnu
cargo test --locked --manifest-path tests/rack-reader-host/Cargo.toml --target x86_64-unknown-linux-gnu
```

## 贡献与来源

维护与 PR 规则见 [Shuyra 贡献规范](https://github.com/shuyra-arche/vanllia/blob/main/CONTRIBUTING.md)。
来源、作者与许可情况见 [PROVENANCE.md](PROVENANCE.md)。
