# Asset Hub

[English](README.md)

## Workspace 管理

VFS 项目包含核心模块 `crates/ah-vfs`，以及两个官方 Driver 插件：
`crates/vfs-driver-local` 和 `crates/vfs-driver-memory`。共享的 `crates/vfs-driver-kit` 提供强类型 SQLite 实例存储、ID 与错误分类。
这四个模块与不发布、仅用于测试的 `crates/vfs-integration-tests` 都属于根 workspace，
共用包配置、lint、`Cargo.lock` 和 `target/`。Local 与 Memory 互不依赖，测试依赖也保持独立；
各自执行 `ah-vfs` 提供的共享存储与实例记录契约断言。跨 Driver 服务与混合挂载场景放在
`vfs-integration-tests` 中，由该模块通过测试依赖装配两个 Driver。
根目录命令统一验证五个模块，且不依赖可选开发工具：

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all --check
```

VFS CI 工作流会在 crate、根 manifest 或工作流变化时，在 Linux 上执行这些检查。
宿主提供数据库连接池，并在 `DriverCatalog` 中注册这些以 Rust crate 提供的官方
Driver 插件。LocalDriver 在本地目录中保存文件。MemoryDriver 将实例配置保存在
`memory_instances` 表中，包括 `capacity_size` 和 `capacity_unit`
（`B`、`KiB`、`MiB`、`GiB` 或 `TiB`）。其文件内容是易失的：同一个 Driver 对象
反复打开实例会共享内容；新建 Driver 对象时，即使复用同一个配置数据库，文件内容也从空开始。

## 实例记录

共享的 `crates/vfs-driver-kit` 提供强类型 SQLite 实例存储、ID 生成和错误分类，并加入根 workspace。
`Driver` 通过 `DriverInfo` 声明稳定的创建参数 schema，通过 `InstanceManagement` 选择记录管理方式，
使用 `OpenContext` 打开存储。Local 与 Memory 使用自管的 `InstanceRegistry` 端口，原有 SQLite 表不变。
`InstanceService` 负责对外实例操作及挂载引用检查。`ah-vfs` 的 `testing` feature 提供可复用的一致性测试。

HostManaged 驱动提供 `InstanceProvisioner`，宿主通过 `MountRuntime::with_host_instances`
注入 `InstanceRecordRepository` 与可选的 `SecretCipher`。host 记录将 schema 的 `config_version`
与每次更新递增的 `revision` 分开；更新保持 `x-immutable` 字段不变，允许修改已挂载实例的其他字段。
打开实例前执行独立迁移并提供解密后的参数；查询记录不打开存储。
`x-secret` 值在持久化前加密，公开详情及 Debug 脱敏；没有 cipher 时明确拒绝创建含 secret 的实例。
当前不提供内置 cipher，认证加密、密钥和 nonce 由宿主管理。
注解遍历支持嵌套 properties、数组 items 和 schema 形式的 additionalProperties；无法处理的引用或组合结构明确拒绝。
配置输入采用不可变且 Debug 脱敏的 `InstanceConfig` 包装，普通 map 可用 `.into()` 转入。

## Driver 初始化设置

Driver 全局 `DriverSettings`、宿主配置和每实例 `InstanceConfig` 是独立的配置层。
可选的 `DriverInfo::settings_schema()` 借用稳定的 JSON Schema 元数据，支持 `x-secret` 注解。
通过 `DriverCatalog::register(driver, settings).await` 在注册前执行 `initialize`；
重复 key 在初始化前拒绝，失败或取消不登记 Driver，Catalog 不保留 settings。
已完成装配的内置驱动通过默认初始化器接受空 settings；声明 settings 的驱动必须实现初始化，
可连接自己的数据库（包括 PostgreSQL），无需接收宿主的 SQLite pool。
配置命名空间标记已改为 `DriverConfigNamespace`，TOML 键仍为 `vfs.driver`，
已有的 `vfs.driver.local` 配置保持兼容。

## VFS 条目访问

三个 VFS Service 共享同一个 `Arc<MountRuntime>`，保留已校验的挂载表，并按
`(DriverKey, InstanceId)` 缓存已校验的 Storage 句柄。挂载变更使挂载表失效，实例删除
使对应句柄失效，包括写入结果不确定或被取消的情况。Driver 打开实例仍属于冷路径。
外部持久化数据变更后需要调用 `MountRuntime::invalidate()`；外部实例配置变更可使用
`invalidate_instance()`。失效会释放缓存持有的句柄，不会撤销已经开始的操作或流。
EntryService 从当前运行时快照中解析最深的挂载，保护挂载点及包含挂载的目录不受文件变更影响，并拒绝
跨挂载移动，即使两处挂载引用同一实例。`EntryService::remove_entry` 只删除文件、链接本身和空目录，不跟随
符号链接。已打开的流保留原来的后端，挂载变化不会撤销这些流。命名空间变更与文件
I/O 之间不进行串行协调。

## 存储能力与错误

`DriverInfo::capabilities()` 声明驱动实现的能力上限；`Storage::capabilities()` 返回已打开
实例及当前平台支持的能力。声明涵盖顺序/范围读取、暂存创建/替换写入、原子发布、目录
表示及空目录持久化、允许/禁止覆盖的原子重命名，以及类型明确的存储属性。它们描述支持
的语义，不代表调用方授权，也不保证所有路径或文件系统上的请求都成功。EntryService
校验声明，并在调用后端前拒绝不支持的请求。基于前缀的适配器必须明确空目录及文件/前缀
冲突行为；复制后删除不能满足原子重命名契约。

`StorageWriteFileRequest::require_atomic_publish` 及对应 VFS 请求字段可显式要求完整文件
原子发布。不支持该保证时，在创建暂存数据前返回错误。原子发布不代表断电持久性。
`StoragePermissions` 提供经过校验的 POSIX 权限位；`StorageXattrs` 提供有大小上限的不透明
属性值及原子创建/替换策略。LocalDriver 在 Unix 上实现权限位，在 Linux/Android 上实现
xattr（取决于文件系统支持）。属性操作使用固定的句柄，不跟随符号链接，并要求具备以
读取方式打开目标的访问权限。VFS 属性变更保留挂载点及其祖先保护。其他平台声明相应
扩展不受支持。

`domain::{StorageError, StorageErrorKind, DriverError}` 是实例打开与 I/O 共用的权威错误
模型；这些类型统一从 `domain` 导入。通过 `StorageError::kind()` 或
`EntryError::storage_error()` 判断类别，无需匹配后端 SDK 类型或错误字符串。原始原因
保留在 `Error::source()` 中供可信诊断使用，Display 只显示归一化类别。条目不存在、句柄
陈旧、后端不可用、超时、冲突和发布结果未知保持不同语义，任何类别都不意味着可以自动
重试。具体驱动在适配边界完成映射。目前的具体官方 Driver 插件为 LocalDriver 和
MemoryDriver，当前未提供 RPC/WASM 适配器。

## 许可证

版权所有 © 2026 AzVs。本项目采用 [Apache License 2.0](LICENSE) 许可证。您可以在遵守该许可证条款的前提下使用、修改和分发本项目；该许可证包含明确的专利授权。除适用法律另有要求外，本项目按“现状”提供，不附带任何明示或默示的担保或条件。
