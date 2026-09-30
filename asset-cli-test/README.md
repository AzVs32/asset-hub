# asset-cli-test

用于手动验证已实现的 Service 接口。

```sh
cargo run -p asset-cli-test -- --help
cargo run -p asset-cli-test -- driver list
cargo run -p asset-cli-test -- mount list
cargo run -p asset-cli-test -- --config config.toml mount info /
cargo run -p asset-cli-test -- mount add /cache memory /
cargo run -p asset-cli-test -- mount resolve /cache/file.txt
cargo run -p asset-cli-test -- mount unmount /cache
cargo run -p asset-cli-test -- mount enable /cache
cargo run -p asset-cli-test -- mount list --show-ids
```

可将根目录 `config.example.toml` 复制为 `config.toml` 后修改。
默认读取当前工作目录中的 `config.toml`，文件或字段缺失时使用默认值。
应用启动配置使用 `[asset]` 节。
`asset.root_mount_path` 和 `asset.config_dir` 必须使用绝对路径；相对或空路径会在创建目录前被拒绝。

`runtime.rs` 及其 `config` 子模块独立管理启动选项、配置加载、数据库初始化和 Service 组装，
不依赖 clap，后续可提取到单独的运行时模块。`main.rs` 负责参数解析、命令分发及输出。

执行查询时会创建配置目录并打开或创建 SQLite 数据库（默认 `/asset-hub-conf/asset.db`）。
启动时会创建并验证根目录（默认 `/asset-hub-data`），并确保数据库存在已启用的 `/` 本地挂载。
本项目的 `config.toml` 使用 `/storage/works/asset-hub/data` 和 `/storage/works/asset-hub/conf`，沿用当前项目目录。
根挂载由 `asset.root_mount_path` 配置决定；重复启动保留其 ID，修改配置会更新数据库中的路径，
不会移动原目录中的文件。其他挂载不受影响。未实现的 Service 操作暂不暴露命令。
启动只更新启用的根挂载，不存在时创建一条启用定义；禁用的根挂载定义保持禁用。
根挂载通过 `MountService::resolve_mount` 查询、通过 `MountService::mount` 保存，与普通挂载使用相同的业务校验；runtime 只负责配置和依赖组装，不直接写挂载仓库。
并发首次启动时，如果另一实例已创建根挂载，则重新查询并复用其 ID；其他挂载错误直接返回。
数据库直接使用当前初始 schema，不提供旧版本升级。

`mount list` 和 `mount info` 均展示 `ALLOWS_SUBMOUNTS` 列：内置 `local` 驱动为
`true`，`memory` 驱动为 `false`，未注册的驱动为 `unknown`。runtime 创建并注册内置驱动，
通过 `DriverService::new` 完成注册，并将同一个 `Arc<DriverService>` 注入 `MountService`。
`DriverRegistry` 仅在 `asset-vfs` 内部使用，CLI 和 runtime 无法直接访问。
`driver list` 通过 `DriverService` 展示所有已注册的类型及子挂载声明。
MountService 查询返回包含驱动声明的
`MountInfo`，CLI 负责展示；禁用挂载仍展示该信息，查询时不绑定对应的 backend。
`mount add <VPATH> <DRIVER> <DRIVER_PATH>` 默认启用；加 `--disabled` 可保存暂不可用的 backend。
所有定义都通过驱动的 `validate_path` 校验路径格式，`local` 只接受绝对路径，禁用定义同样受此限制。
例如 `mount add /s local /storage` 将本地 `/storage` 挂载到虚拟路径 `/s`。
启用时校验 backend 根路径、同路径启用挂载唯一性及所有启用祖先的子挂载声明。
`mount enable` 重新启用并保留 ID；`mount unmount` 仅禁用记录，不删除定义或子挂载。启用的根挂载不能卸载。

`info`、`enable` 和 `unmount` 优先使用虚拟路径，也接受 mount ID。
同路径存在启用定义时选择它；没有启用定义且只有一条禁用记录时选择该记录；多条禁用记录需要 ID。
列表默认以虚拟路径标识挂载，同路径多条记录时自动展示 ID；`--show-ids` 可用于 `list` 和 `info` 强制显示 ID。
新增禁用定义或卸载后，如果虚拟路径无法唯一定位该定义，变更结果也会自动展示 ID。
`mount resolve <VPATH>` 返回最深启用挂载及请求的相对路径；禁用挂载被忽略。
