# asset-cli-test

用于手动验证已实现的 Service 接口。

```sh
cargo run -p asset-cli-test -- --help
cargo run -p asset-cli-test -- driver list
cargo run -p asset-cli-test -- mount list
cargo run -p asset-cli-test -- --config config.toml mount info <ID>
```

可将根目录 `config.example.toml` 复制为 `config.toml` 后修改。
默认读取当前工作目录中的 `config.toml`，文件或字段缺失时使用默认值。
应用启动配置使用 `[asset]` 节。
配置中的相对路径基于当前工作目录。

`runtime.rs` 及其 `config` 子模块独立管理启动选项、配置加载、数据库初始化和 Service 组装，
不依赖 clap，后续可提取到单独的运行时模块。`main.rs` 负责参数解析、命令分发及输出。

执行查询时会创建配置目录并打开或创建 SQLite 数据库（默认 `conf/vfs.db`）。
启动时会创建并验证根目录（默认 `data`），并确保数据库存在已启用的 `/` 本地挂载。
根挂载由 `asset.root_mount_path` 配置决定；重复启动保留其 ID，修改配置会更新数据库中的路径，
不会移动原目录中的文件。其他挂载不受影响。未实现的 Service 操作暂不暴露命令。

`mount list` 和 `mount info` 均展示 `ALLOWS_SUBMOUNTS` 列：内置 `local` 驱动为
`true`，`memory` 驱动为 `false`，未注册的驱动为 `unknown`。runtime 创建并注册内置驱动，
通过 `DriverService::new` 完成注册，并将同一个 `Arc<DriverService>` 注入 `MountService`。
`DriverRegistry` 仅在 `asset-vfs` 内部使用，CLI 和 runtime 无法直接访问。
`driver list` 通过 `DriverService` 展示所有已注册的类型及子挂载声明。
MountService 查询返回包含驱动声明的
`MountInfo`，CLI 负责展示；禁用挂载仍展示该信息，查询时不绑定对应的 backend。
该列表示驱动声明；实际子挂载校验尚待挂载操作实现。
