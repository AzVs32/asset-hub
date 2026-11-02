# WASM 插件宿主

插件必须是 WebAssembly Component，保留同步的 `assethub:vfs/driver@2.0.0.call` 导出，控制消息采用 protobuf。宿主提供标准 WASIp2 CLI、时钟、随机数、文件系统、socket 和 HTTP 接口，使用异步 WASI 链接和异步宿主调用。核心模块不接受，也不自动适配。当前 Wasmtime 为 50.0.1，p3 支持暂缓。

## 默认权限

默认没有 HTTP 来源、TCP 目标、预开放目录或环境变量；没有命令行参数，stdin 为 EOF，stdout/stderr 丢弃输出。标准时钟和随机数可用。每个控制连接和 Storage 分别拥有 WasiCtx、资源表和 Store。

`required_host_capabilities` 使用 `http`、`tcp`、`fs.read`、`fs.write`、`env`。插件要求未授权的能力时，在初始化之前拒绝加载，错误链会列出缺少的能力。导入的每个接口仍必须能在 Linker 中找到；声明能力不代替实际的逐次操作检查。

## 配置示例

```toml
[[vfs.plugins.entries]]
transport = "wasm"
module = "plugins/storage.wasm"

[vfs.plugins.entries.grants]
tcp = ["storage.example:443"]
env = ["STORAGE_REGION"]

[[vfs.plugins.entries.grants.fs]]
host = "/srv/plugin-data"
guest = "/data"
write = false

[vfs.plugins.entries.grants.http]
origins = ["https://storage.example"]
max_request_bytes = 8388608
max_response_bytes = 8388608
max_headers = 64
timeout_ms = 20000
allow_redirects = false
# 内部 DNS 服务若确实需要私有地址，逐个显式授权：
private_addresses = []

[vfs.plugins.entries.grants.limits]
memory_bytes = 134217728
fuel = 2000000000
timeout_ms = 30000
storage_instances = 16
control_instances = 4
```

- **HTTP**：来源按 scheme、主机和有效端口匹配，不允许路径、凭证、查询或片段。钩子逐次检查来源、头数量、请求/响应体大小和总超时；请求体完整校验后才发送。DNS 每次重新解析，检查全部地址后固定本次连接的解析结果，禁用代理和自动重试。DNS 名称默认不能指向特殊地址；显式 IP 来源或 `private_addresses` 可授权内部服务。HTTP 授权不会开放原始 TCP 或 guest DNS。
- **重定向**：默认拒绝。启用后最多跟随五次，每一跳重新检查来源和地址；跨来源移除 Authorization/Cookie。超时涵盖整个跳转链。
- **TCP/DNS**：只接受明确的 `host:port`。标准 WASI DNS 入口先检查域名白名单；`socket_addr_check` 在连接时检查目标端口、实际 IP 和当前 DNS 回答。回环、私有、链路本地、CGNAT、元数据及特殊网段默认拒绝；要允许它们必须列出字面 IP:port，单独允许域名不会隐式允许其私有解析结果。只允许外连所需的临时本地绑定，拒绝监听、接受连接及 UDP。
- **文件**：每个目录指定绝对宿主路径、绝对 guest 路径和 `write`。`false` 是只读，`true` 是读写。WASI 的目录能力约束路径遍历和越界符号链接；没有继承宿主根目录或 cwd。
- **环境变量**：只复制白名单中的现有变量，不继承整个进程环境；不提供参数或真实 stdio。

## 限额与失败

默认每个 Store 的 WASM 线性内存总量为 128 MiB，单次调用燃料为 20 亿、墙钟超时为 30 秒。燃料、epoch 约束 WASM 计算；阻塞的宿主 I/O 必须依靠墙钟超时。超时或 WASM 执行陷阱会丢弃该 Store、归还并发名额并关闭其资源，其他 Storage 保持可用；已经开始的变更操作可能返回未知结果，不能自动重放。

HTTP 请求和响应默认各限 8 MiB，头数量为 64，HTTP 超时为 20 秒。HTTP 缓冲、宿主资源和编译代码的内存不属于 WASM 线性内存计数，需结合体积与并发限额控制总内存。

WebDAV 继续保留最大 8 MiB 的提交前暂存限制：当前实现要等 commit 才发出整个 PUT，且宿主在发送前完整校验请求体。取消限制需要独立的、受限的暂存或上传协议设计。S3 继续按 multipart 分片控制内存。

## 构建与验证

```sh
rustup target add wasm32-wasip2
scripts/build-wasi-plugins.sh --offline
scripts/build-wasi-probe.sh --offline
cargo test -p vfs-plugin-wasm -- --nocapture
```

独立插件可直接 `cargo build --locked --target wasm32-wasip2`，产物已经是组件。Rust guest SDK 的 HTTP 客户端直接调用 `wasip2` 的标准 HTTP 绑定，使用标准 Rust `http::Request/Response`，不再引入自定义宿主 WIT 服务；时间使用 `SystemTime`。选择直接绑定是因为驱动导出仍是同步调用，这样能使用标准 pollable 阻塞等待，而无需额外 guest 异步运行时和客户端依赖。标准 HTTP 接口可流式传输，但当前 SDK 和宿主策略均收集有界的请求/响应体，尚未提供端到端流式上传。

测试夹具直接使用正式的 plugin.wit，仅导出驱动接口。默认自动发现上述脚本构建的组件，可通过 `VFS_WASI_TEST_COMPONENT` 指定其他路径。测试通过真实 Rust 组件执行文件、环境、TCP、HTTP、限额及取消测试。DNS 重绑定测试在宿主测试构建中控制标准 WASI DNS 回答，使组件先得到公有 IP、再得到回环 IP，确认实际连接被拒；生产构建没有该注入入口。缺少工具或组件产物时测试打印 SKIP 原因，不使整个测试套件失败。

当前仅支持 WASIp2。增加 p3 支持前，需要解决同步导出等待异步接口可能出现的 `CannotBlockSyncTask`，并使用真实组件验证。JS 组件尚未验证。
