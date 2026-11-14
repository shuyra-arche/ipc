# Twine · 纺

Twine 是 Shuyra 的 `no_std` 通信库，使用 Kernel ABI v11 并适配
[Weave](https://github.com/shuyra-arche/weave) 的完整帧接口。

| 模块 | 职责 |
| --- | --- |
| `view::clone` | 有界消息、独立接收载荷及单次请求的回复授权 |
| `view::share` | 本地和跨 Team 的 SPSC 字节环 |
| `view::memory` | 独立 CPU 预留、原生关联与发布、保留请求及显式回收 |
| `sign` | 就绪事件订阅、绝对截止时间等待及合并通知 |

## 原生内存上下文

API、Client 和测试镜像的 Server 固定到 Kernel
[`26.11.14-preview-Gamma`](https://github.com/shuyra-arche/kernel/releases/tag/26.11.14-preview-Gamma)，
对应提交 `9a35dd5420474ebe75a6b5f3e4589a398745e32c`。
必须重建匹配的 Kernel、Capsule 和 Ledger；不再使用 `memory-v11` feature。

ABI v11 的 Share 只提供逻辑来源，旧 `Open/Shut` 调用已退役。
映射需要当前 Task 自己的 Internal Nest 上的 Reserve 授权。
用 `view::memory::Native::new(space, nest, requests)` 构造上下文：
这是 unsafe 构造，因为外部 Nest 上发布成功不代表地址在调用方可访问。
`requests` 是 `FnMut() -> Option<RequestId>`，必须接入该 Task 所有原生内存调用
共用的单调递增序列；耗尽返回 `None`，不可为每个端点另起序列或重置。
可用 `&mut Native` 独占借用，或用 `&RefCell<Native>` 在同 Task 的端点间共享。

每个映射拥有独立 Nest Reservation，通过 `Reserve → Insert → Publish` 建立
CPU 访问。Share 页坐标沿用授权观察报告的原始 View 起点，收窄窗口不重置坐标。
映射按需建立，后续操作检查 Memo 的发布状态。清理通过原生 Release 结清整个
Reservation，确认完成后才归还其 Pend；借用的 Share 来源由调用方回收。

映射保留未结清操作的完整描述符和 RequestId。交付失败或 Pending 后重试时，
重放同一请求，由 Kernel 的 Task journal 去重；不会用新编号重复执行已接受的
操作。Rejected Busy 可以重新申请编号重试。`MappingError::Uncertain`、
`Pending`、`Failed` 和 `Expired` 携带 `Receipt`，可用 `Receipt::query()`
查询原结果。Failed 或 Expired 不表示回滚或硬件结清，保留的 owner 不会自动
丢弃该记录；运行时需检查原结果并处理未结清资源，不能以超时代替完成证据。

## Clone 与 Sign

用已授权的 Clone Pend 构造 `Sender::new(token)` 或
`Receiver::new(token, memory)`。消息必须非空。发送成功表示已入队。
接收缓冲不足时，Kernel 保留队头；`peek` 只观察队头，另一个接收者仍可先行取走它。

直接使用 `pull` 时，调用方负责结束 `Payload`。载荷可以在原队列关闭后读取。
`Payload::copy(out, memory)` 通过独立的只读 CPU 预留复制完整载荷，
不会向外暴露可跨 Kernel 调用存活的 Rust slice。
`shut(memory)` 释放 CPU 预留并保留载荷来源，可再次复制；
`finish(memory)` 先结清预留，再归还载荷 Pend。两者都借用可变载荷，失败后可重试。
手动 Pull 后可用 `Receiver::memory()` 借用原上下文，所有操作必须留在同一 Task。

Weave 的 `FrameReceiver` 适配器在映射 Busy、Pending 或交付结果不确定时保留
已出队载荷，下一次接收优先处理该载荷。复制完成即交付帧。回收失败时保留
待归还的载荷，下次 Pull 前先重试，也可显式调用 `settle`。
放弃未交付载荷或停止服务时，调用 `discard`；它不会接收下一帧或关闭借用的队列。
显式清理可以观察失败。析构只尽力清理；手动 Payload 析构无法借用内存上下文，
未结清 CPU 预留可能保留到 Task 退出，因此应显式调用 `finish`。

用 `sign::Watch` 订阅 Clone 的 `source()`，其中 Clone Pend 需要 `PEEK`。
使用 Kernel 的 `Wait::At` 传入绝对单调纳秒。唤醒后应重新尝试实际操作。
Push 就绪只表示队列还有空间，未承诺能放入任意长度的帧。

## 请求与回复

请求方用 `Reply::create(license, max_len, memory)` 创建单次回复队列，调用
`grant` 授予服务 Task 不可再分发的 Push 权限，再把服务本地 Pend 编号编码进请求。
Kernel 按活授权谱系检查委托，不要求双方同 Team 或有直接创建者关系；
Dormant Task 也能接收初始授权。每个接收 Task 的外来登记上限为 256，
其中同一发送 Task 最多 32、同一发送 Team 合计最多 128；总登记上限为 1024，
含准备中的登记。超额返回 OutOfMemory，Busy 可在有界期限内重试。
接收方 Return、来源撤销或退出在清理结清后释放额度。资源委托不授予执行管理权。

服务方用 `Sender::reply` 将编号与 Clone 返回的可信发送者身份核对，再把
Sender 交给 Weave 的回复路由。`Reply` 实现 `ReplyReceiver`，验证 Kernel 报告的
回复来源，只接受一次成功回复。请求完成、失败或超时后都应调用 `Reply::finish`，
失败时重试清理。等待超时只结束本次等待，服务可能已执行请求。

## Share 字节环

`Ring::new` 接受调用方的原子字节存储，`split` 提供唯一读端和写端。
读写可以部分推进，环空或满时返回 `Pending`。写端关闭后读端排空返回 `Eof`，
读端关闭后写端返回 `BrokenPipe`。字节环的帧边界由使用者定义。

跨 Team 时，通过原生 Flat Create/Occupy 为数据、读游标、写游标分别准备清零的
4 KiB BASE Cell RAM，再通过 Share Derive 和 Pend Accord 独立授权。两端以相同容量、各自 Task
的内存上下文和 Share `Pages` 构造 `MappedReader::new(memory, pages, capacity)`
或 `MappedWriter::new(memory, pages, capacity)`。每个来源需要 `ASSOCIATE`。

| 区域 | 读端 | 写端 |
| --- | --- | --- |
| 数据 | 只读 | 读写 |
| 读游标 | 读写 | 只读 |
| 写游标 | 只读 | 读写 |

这两个构造函数是 unsafe：调用方必须保证唯一读写端、原子布局、互不重叠的存储和
每次访问期间的映射有效性。Memo 观察不是生命周期锁；应在端点停止访问后协调
撤销或物理存储变更。`MappingError::is_busy()` 表示 Busy 或 Pending；重试仍使用
原端点。本次映射失败时尚未推进字节。`close` 发布端点关闭标记，`finish` 结清
CPU 预留，失败后可重试；析构不访问共享页，只尽力回收预留。

为每端配置专用接收 Sign 和对端 Trigger Pend，用 `sign::Wake` 在推进或关闭后
通知对端。收到直接通知后重试读写，通知失败与字节进度分别处理。有观察权限的
一方可在接收 Sign 上订阅对端 Join 或相关 Pend 的 Inspect，在对端退出或授权
失效时结束等待。`Eof` 表示写端已关闭且数据已排空。

## 构建与检查

库与宿主测试固定 Rust 1.99.0，保留全部 `Cargo.lock` 以复现验证。
宿主测试覆盖环算法以及原生请求重放、清理顺序、固定来源坐标和撤销后的停止访问；
脚本化内存 transport 不证明真实 Kernel 调用。

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --target x86_64-unknown-linux-gnu
cargo build --locked --target riscv64gc-unknown-none-elf
cargo build --locked --release --target riscv64gc-unknown-none-elf
```

真实 RV64 测试使用同一 Gamma tag 的 RAM-only 测试平台，Server 构建另外需要
nightly。准备 QEMU 10.0.x 和支持 DBCN 的 OpenSBI，然后运行：

```sh
rustup toolchain install nightly-2026-11-14 --profile minimal --target riscv64gc-unknown-none-elf
export QEMU_RISCV64=qemu-system-riscv64
export QEMU_OPENSBI=/usr/lib/riscv64-linux-gnu/opensbi/generic/fw_dynamic.bin
python3 tests/target/share/run.py
python3 tests/target/share/run.py --profile debug
```

可选的位置参数指定已经构建的匹配测试镜像；`--no-build` 复用本地二进制。
测试覆盖跨页 Clone 载荷、队列关闭后读取、释放后重新映射、单次回复、两个独立
Team 的满环背压、回绕、排空、等待期间的撤销及对端退出。
`tests/target/share/target/evidence/` 保存镜像校验、命令、串口和退出结果。
Gamma 的 native 内存契约只接受此处的单 Hart 发布；旧四 Hart 用例不属于本次
ABI v11 验收。未声明 SMP 或物理硬件验证。

Twine 0.4 是从 0.3 / ABI v10 的不兼容迁移：接收、回复及共享环需要显式传入
内存上下文；载荷的 `copy/shut/finish` 签名和映射错误类型也已更新。
API、Client、Server、Capsule、Ledger 及使用方须一起重建。
来源与归属见 [PROVENANCE.md](PROVENANCE.md)。
