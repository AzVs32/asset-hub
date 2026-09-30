# asset-cli-test

用于手动验证已实现的 Service 接口。

```sh
cargo run -p asset-cli-test -- --help
cargo run -p asset-cli-test -- mount list
cargo run -p asset-cli-test -- --config config.toml mount info <ID>
```

可将根目录 `config.example.toml` 复制为 `config.toml` 后修改。
默认读取当前工作目录中的 `config.toml`，文件或字段缺失时使用默认值。
配置中的相对路径基于当前工作目录。

`runtime.rs` 独立管理启动选项、配置加载、数据库初始化和 Service 组装，
不依赖 clap，后续可提取到单独的运行时模块。`main.rs` 负责参数解析、命令分发及输出。

执行查询时会创建配置目录并打开或创建 SQLite 数据库（默认 `conf/vfs.db`）。
启动时会创建并验证根目录（默认 `data`），并确保数据库存在已启用的 `/` 本地挂载。
根挂载由 `vfs.root_mount_path` 配置决定；重复启动保留其 ID，修改配置会更新数据库中的路径，
不会移动原目录中的文件。其他挂载不受影响。未实现的 Service 操作暂不暴露命令。
