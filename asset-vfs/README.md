# asset-vfs

VFS 核心定义虚拟命名空间、挂载领域对象、扩展接口和 Service。具体驱动、数据库和启动配置由应用组装。

| 组件 | 职责与边界 |
| --- | --- |
| `DriverService` | 对外查询驱动类型与声明；构造时通过 `Driver` 接口注册工厂；查询不绑定 backend、不读写挂载存储。 |
| `DriverRegistry` | crate 内部的内存索引；按 `DriverKind` 查找工厂、拒绝重复注册；不参与持久化、挂载拓扑或配置加载。 |
| `MountService` | 挂载用例；查询仓库中的定义，通过共享的 `DriverService` 补充驱动声明；不识别具体驱动类。 |
| `VfsService` | 虚拟文件操作的入口，目前尚为占位 API。 |
| `Driver` / `BoundDriver` | 驱动扩展接口；实现负责解析 `DriverPath`、绑定并限制 backend 根目录、提供读写能力。 |
| `MountRepository` | 挂载定义的持久化接口；不绑定驱动、不解释驱动路径、不执行挂载业务规则。 |
| runtime / infra | runtime 选择与组装实现、加载 TOML、准备本地目录并确保默认根挂载；infra 实现驱动与仓库。 |

外部业务调用使用 Service。`Driver` 和 `MountRepository` 保持公开，供适配器实现以及 runtime 注入依赖；这两个扩展接口不依赖具体实现。

runtime 将 `Vec<Arc<dyn Driver>>` 传入 `DriverService::new`，注册失败会返回错误。构造完成后注册集合不可变。`DriverService` 按实例持有注册表，通过 `Arc<DriverService>` 与 `MountService` 共享；没有全局单例。

`DriverService` 提供 `list()`、`info(kind)`、`require(kind)`、`len()` 和 `is_empty()`。`list` 返回排序后的 `DriverKind` 快照；`info` 和 `require` 返回只读 `DriverInfo`，其中包含类型与是否允许子挂载。未知类型分别返回 `None` 或 `UnregisteredKind`。这些 API 不返回驱动工厂或注册表引用。

`Mount` 只保存定义，`MountInfo` 保存查询时派生的声明。未注册类型的子挂载声明为 `None`，CLI 显示 `unknown`。驱动是否可绑定与声明查询分开；禁用挂载和暂不可用的 backend 仍可查询。

边界检查已覆盖核心的所有生产模块：领域类型仅做虚拟路径、标识和领域值处理；Service 依赖扩展接口，不引用 `LocalDriver`、`MemoryDriver` 或 SQLite。原先公开的注册表已收为 crate 内部，原先包含 `PathBuf`、数据库选择及 `sqlite_path()` 的启动配置已移到 `asset-cli-test/src/runtime/config.rs`，配置节为 `[asset]`，字段和启动默认值保持不变。

`MountService::mount/unmount/resolve_mount` 和 `VfsService` 操作仍为占位 API。后续实现挂载写操作时，应在 MountService 中执行子挂载规则；仓库只保存定义。根路径 `/` 使用普通 `Mount`，启动时必须存在且默认选择 local 是应用策略。
