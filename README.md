# Twine · 纺

Twine 是 Shuyra 的 `no_std` 通信库，使用 Kernel ABI v7 并适配
[Weave](https://github.com/shuyra-arche/weave) 的完整帧接口。

| 模块 | 职责 |
| --- | --- |
| `view::clone` | 有界消息、独立接收载荷及单次请求的回复授权 |
| `view::share` | 同一地址空间的 SPSC 字节环 |
| `sign` | 就绪事件订阅、绝对截止时间等待及合并通知 |

## Clone 与 Sign

用已授权的 Clone Pend 构造 `Sender` 或 `Receiver`。Kernel ABI v7
要求消息非空。发送成功表示已入队。接收缓冲不足时，Kernel 保留队头；
`peek` 只观察队头，另一个接收者仍可先行取走它。

直接使用 `pull` 时，调用方负责 `Payload::finish`。载荷可以在原队列关闭后
读取。`Payload::copy` 在操作内访问映射，`shut` 提前关闭映射，
`finish` 归还 Pend 并关闭剩余映射。清理失败返回仍可重试的对象。
Weave 的 `FrameReceiver` 适配器在 Open 返回 `Busy` 时保留已出队载荷，
下一次接收优先处理该载荷。复制完成即交付帧。回收失败时，适配器保留
待归还的 Pend，在下次 Pull 前重试，也可显式调用 `settle`。析构只尝试
一次清理。需要观察结果的调用方须显式结束资源。

用 `sign::Watch` 订阅 Clone 的 `source()`，其中 Clone Pend 需要
`PEEK`。使用 Kernel 的 `Wait::At` 传入绝对单调纳秒。唤醒后应重新尝试
实际操作。Push 就绪只表示队列还有空间，未承诺能放入任意长度的帧。

## 请求与回复

请求方用 `Reply::create` 创建单次回复队列，调用 `grant` 先授予服务 Task
不可再分发的 Push 权限，再将返回的服务本地 Pend 编号编码进请求。
Kernel 只允许同 Team 或符合直接创建者关系的 Task 之间授予。
服务方用 `Sender::reply` 将编号与 Clone 返回的可信发送者身份核对，
再将该 Sender 交给 Weave 的回复路由。请求方的 `Reply` 实现
`ReplyReceiver`，验证 Kernel 报告的回复来源，只接受一次成功回复。
请求完成、失败或超时后都应调用 `Reply::finish`，失败时可以重试清理。
等待超时只结束本次等待，服务可能已执行请求。

## Share 字节环

`Ring::new` 接受调用方的原子字节存储，`split` 提供唯一的读端和写端。
读写可以部分推进，环空或满时返回 `Pending`。写端关闭后读端排空返回
`Eof`，读端关闭后写端返回 `BrokenPipe`。字节环的帧边界由使用者定义。

当前 ABI 的 Share 写借用是独占的。授出只读分支会撤销父分支的写映射，
因此这版字节环的两个端点必须处在同一个地址空间。跨隔离域的三个独立授权区
需要 Kernel 能对同一页同时保留一个写端与一个只读端的映射。

## 构建与检查

仓库固定 Rust 1.99.0，保留 `Cargo.lock` 以复现验证。依赖按完整 Git 提交
固定。宿主测试运行字节环，RV64 构建检查目标兼容性。原生 Clone 和 Sign
通过 Kernel Client 调用，需要在 Shuyra 的 Task 中运行。

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --target x86_64-unknown-linux-gnu
cargo build --locked --target riscv64gc-unknown-none-elf
cargo build --locked --release --target riscv64gc-unknown-none-elf
```

Twine 0.2 替换原 IPC 0.1 的接口，使用它的程序须重新编译并迁移到
Kernel ABI v7。来源与归属见 [PROVENANCE.md](PROVENANCE.md)。
