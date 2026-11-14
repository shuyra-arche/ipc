# Twine · 纺

Twine 是 Shuyra 的 `no_std` 通信库，使用 Kernel ABI v10 并适配
[Weave](https://github.com/shuyra-arche/weave) 的完整帧接口。

| 模块 | 职责 |
| --- | --- |
| `view::clone` | 有界消息、独立接收载荷及单次请求的回复授权 |
| `view::share` | 本地和跨 Team 的 SPSC 字节环 |
| `sign` | 就绪事件订阅、绝对截止时间等待及合并通知 |

## Clone 与 Sign

用已授权的 Clone Pend 构造 `Sender` 或 `Receiver`。Kernel ABI v10
要求消息非空。发送成功表示已入队。接收缓冲不足时，Kernel 保留队头；
`peek` 只观察队头，另一个接收者仍可先行取走它。

直接使用 `pull` 时，调用方负责 `Payload::finish`。载荷可以在原队列关闭后
读取。`Payload::copy` 在操作内访问映射，`shut` 提前关闭映射，
`finish` 归还 Pend 并关闭剩余映射。清理失败返回仍可重试的对象。
Weave 的 `FrameReceiver` 适配器在 Open 返回 `Busy` 时保留已出队载荷，
下一次接收优先处理该载荷。复制完成即交付帧。回收失败时，适配器保留
待归还的 Pend，在下次 Pull 前重试，也可显式调用 `settle`。析构只尝试
一次清理。放弃尚未映射的载荷或停止服务时，调用 `discard` 归还适配器持有的
载荷，失败后可重试。它不会接收下一帧，也不会关闭借用的队列。
需要观察结果的调用方须显式结束资源。

用 `sign::Watch` 订阅 Clone 的 `source()`，其中 Clone Pend 需要
`PEEK`。使用 Kernel 的 `Wait::At` 传入绝对单调纳秒。唤醒后应重新尝试
实际操作。Push 就绪只表示队列还有空间，未承诺能放入任意长度的帧。

## 请求与回复

请求方用 `Reply::create` 创建单次回复队列，调用 `grant` 先授予服务 Task
不可再分发的 Push 权限，再将返回的服务本地 Pend 编号编码进请求。
Kernel 按活授权谱系检查委托，不要求双方同 Team 或有直接创建者关系；
未运行的 Dormant Task 也能接收初始授权。每个接收 Task 的外来登记上限为 256，
其中同一发送 Task 最多 32、同一发送 Team 合计最多 128；总登记上限为 1024，
含准备中的登记。超额返回 OutOfMemory，Busy 可在有界期限内重试。
接收方 Return、来源撤销或退出在清理结清后释放额度。资源委托不授予执行管理权。
服务方用 `Sender::reply` 将编号与 Clone 返回的可信发送者身份核对，
再将该 Sender 交给 Weave 的回复路由。请求方的 `Reply` 实现
`ReplyReceiver`，验证 Kernel 报告的回复来源，只接受一次成功回复。
请求完成、失败或超时后都应调用 `Reply::finish`，失败时可以重试清理。
等待超时只结束本次等待，服务可能已执行请求。

## Share 字节环

`Ring::new` 接受调用方的原子字节存储，`split` 提供唯一的读端和写端。
读写可以部分推进，环空或满时返回 `Pending`。写端关闭后读端排空返回
`Eof`，读端关闭后写端返回 `BrokenPipe`。字节环的帧边界由使用者定义。

跨 Team 时，为数据、读游标、写游标分别分配清零的 RAM。两端以相同容量和各自
Task 的 `Pages` 构造 `MappedReader` 或 `MappedWriter`。每个区域独立授权。

| 区域 | 读端 | 写端 |
| --- | --- | --- |
| 数据 | 只读 | 读写 |
| 读游标 | 读写 | 只读 |
| 写游标 | 只读 | 读写 |

授权方负责创建、分发和回收 Pend。映射端点会在每次操作开始时调用 Share
Open，访问期间需要映射保持有效。应在端点停止访问后协调撤销。映射端点
需要显式 `close`，析构时无需访问共享页。`MappingError::is_busy()` 表示
Open 暂时受争用，环状态与本次字节进度均未改变，可以重试。

为每端配置专用的接收 Sign 和对端的 Trigger Pend，用 `sign::Wake` 在
推进或关闭后通知对端。收到直接通知后重试读写，通知失败与字节进度分别处理。
有观察权限的一方可以在接收 Sign 上订阅对端的 Join，或订阅相关 Pend 的
Inspect，在对端退出或授权失效时结束等待。`Eof` 表示写端已关闭且数据已排空。

本版本固定 Kernel/API/Client `8b3586206a4795fc8289031887bb62d6d7598a69`；
跨 Team 委托需要该修订或兼容后继，不能仅替换旧运行镜像的依赖针脚。

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

有 ABI v10 的 Shuyra QEMU 启动镜像时，运行
`python3 tests/target/share/run.py /path/to/shuyra`。这个测试在单 Hart 和四 Hart
下启动两个独立 Team，覆盖满环背压、回绕、排空、等待期间的撤销及对端退出。
可通过 `QEMU_RISCV64` 和 `QEMU_OPENSBI` 指定 QEMU 和固件。

Twine 0.3 从 Twine 0.2 / Kernel ABI v9 迁移到 ABI v10，使用它的程序须重新编译并迁移到
Kernel ABI v10。来源与归属见 [PROVENANCE.md](PROVENANCE.md)。
