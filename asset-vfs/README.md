# asset-vfs

VFS 核心定义虚拟命名空间、挂载领域对象、扩展接口和 Service。具体驱动、数据库和启动配置由应用组装。

| 组件 | 职责与边界 |
| --- | --- |
| `DriverService` | 对外查询驱动类型与声明；构造时通过 `Driver` 接口注册工厂；查询不绑定 backend、不读写挂载存储。 |
| `DriverRegistry` | crate 内部的内存索引；按 `DriverKind` 查找工厂、拒绝重复注册；不参与持久化、挂载拓扑或配置加载。 |
| `MountService` | 挂载、卸载、查询和路径解析；在事务内校验启用路径唯一性与子挂载规则，通过 `DriverService` 校验 backend；不识别具体驱动类。 |
| `VfsService` | 虚拟文件操作的入口，目前尚为占位 API。 |
| `Driver` / `BoundDriver` | 驱动扩展接口；实现负责校验与解析 `DriverPath`、绑定并限制 backend 根目录、提供读写能力。 |
| `MountRepository` / `MountTransaction` | 挂载定义的持久化及事务接口；事务串行化写入并保持一致快照，不解释驱动声明；业务规则由 service 执行。 |
| runtime / infra | runtime 选择与组装实现、加载 TOML、准备本地目录并确保默认根挂载；infra 实现驱动与仓库。 |

外部业务调用使用 Service。`Driver` 和 `MountRepository` 保持公开，供适配器实现以及 runtime 注入依赖；这两个扩展接口不依赖具体实现。

runtime 将 `Vec<Arc<dyn Driver>>` 传入 `DriverService::new`，注册失败会返回错误。构造完成后注册集合不可变。`DriverService` 按实例持有注册表，通过 `Arc<DriverService>` 与 `MountService` 共享；没有全局单例。

`DriverService` 提供 `list()`、`info(kind)`、`require(kind)`、`len()` 和 `is_empty()`。`list` 返回排序后的 `DriverKind` 快照；`info` 和 `require` 返回只读 `DriverInfo`，其中包含类型与是否允许子挂载。未知类型分别返回 `None` 或 `UnregisteredKind`。这些 API 不返回驱动工厂或注册表引用。

`Mount` 只保存定义，`MountInfo` 保存查询时派生的声明。未注册类型的子挂载声明为 `None`，CLI 显示 `unknown`。驱动是否可绑定与声明查询分开；禁用挂载和暂不可用的 backend 仍可查询。

边界检查已覆盖核心的所有生产模块：领域类型仅做虚拟路径、标识和领域值处理；Service 依赖扩展接口，不引用 `LocalDriver`、`MemoryDriver` 或 SQLite。注册表仅供 crate 内部使用，包含 `PathBuf`、数据库选择及 `sqlite_path()` 的启动配置位于 `asset-cli-test/src/runtime/config.rs`，配置节为 `[asset]`，默认根目录为 `/asset-hub-data`，默认数据库路径为 `/asset-hub-conf/asset.db`；配置中的存储目录必须使用绝对路径。

`MountService::mount(Mount)` 新增定义或按 ID 更新定义，重新启用时保留 ID。只有启用挂载的虚拟路径必须唯一，同路径允许多个禁用定义。所有定义都由驱动的 `validate_path` 校验路径格式，该校验不访问 backend；`LocalDriver` 只接受绝对路径。启用前进一步绑定 backend 校验根路径是否可用，禁用定义可以指向暂不可用的绝对目录。所有启用的祖先挂载都必须允许子挂载，禁用祖先不施加限制；向已有子挂载上方插入父挂载也会检查规则。

`mount_info` 和 `unmount` 接收 `MountSelector`，可由 `VirtualPath` 或 `MountId` 转换。按精确路径优先选择启用挂载；没有启用挂载时，单个禁用定义可直接定位，多个禁用定义返回 `AmbiguousPath`，需要 ID。`unmount` 将定义设为禁用，保留 ID、存储记录和子挂载；已禁用定义可重复卸载。启用的根挂载不能禁用或移到其他位置。

`resolve_mount(&VirtualPath)` 选择覆盖请求路径的最深启用挂载，按完整路径段匹配，返回拥有挂载快照及相对路径的 `ResolvedMount`。它不绑定 backend，未覆盖时返回 `None`。

已实现的挂载 Service 方法与 CLI 对应如下：

| Service 方法 | CLI 使用 |
| --- | --- |
| `MountService::new` | runtime 注入仓库和共享的 `DriverService`。 |
| `mount` | `mount add` 保存定义；`mount enable` 启用已有定义；runtime 保存配置决定的根挂载。 |
| `unmount` | `mount unmount <VPATH_OR_MOUNT_ID>` 禁用定义。 |
| `list_mounts` | `mount list` 展示定义；变更输出据此判断禁用定义是否需要显示 ID。 |
| `mount_info` | `mount info` 查询定义；`mount enable` 读取待启用定义。 |
| `resolve_mount` | `mount resolve <VPATH>` 解析路径；runtime 查找当前启用的根挂载。 |

`driver list` 调用 `DriverService::is_empty`、`list` 和 `require`。`info` 用于 `MountService` 派生驱动声明，`len` 提供数量查询并被 `is_empty` 使用。这些查询均已实现。

SQLite 初始 schema 在 `infra/migrations/sqlite/0001_create_mounts.sql` 中直接定义，仅对启用记录建立路径唯一索引。只支持当前模型，不提供旧数据库升级。挂载业务在仓库写事务中校验并提交，跨 repository/service 实例的并发写入也受事务保护。

`VfsService::list/read/write/mkdir/remove/rename` 均未实现，没有对应 CLI 命令。`ReadDriver::list` 是已实现的驱动层能力，尚未组装为 VFS 的目录查询；`WriteDriver` 仍只有能力接口。

根路径 `/` 使用普通 `Mount`。runtime 准备本地目录并选择默认 local 驱动，通过 `resolve_mount` 查找启用根挂载，再调用 `mount` 完成保存与业务校验；不直接写仓库。仓库的 `insert/remove` 是底层存储接口，其中 `remove` 物理删除定义，业务卸载统一使用 `MountService::unmount`。
