# Asset Hub

Asset Hub 是一个以本地为主的资源管理系统。它通过 Web 页面和本地管理命令对外提供一个全局资源
工作区，并以六边形架构围绕两个聚合构建：

- `Resource`：一个资源及其元数据与内容引用。
- `Directory`：由稳定 UUID 标识的独立层级节点。

## 前置条件

- Rust（固定工具链见 [rust-toolchain.toml](rust-toolchain.toml)，`rustup` 会自动安装）。
- Web 页面需要 Node.js 22.12 或更高版本。

## 快速开始

可将 [config.example.toml](config.example.toml) 复制为 `config.toml` 后按需修改。不提供配置文件
时使用内置默认配置：Blob 存储根目录为 `data/`，SQLite 数据库位于
`data/.asset-hub/asset-hub.sqlite`。

安装 Web 应用依赖：

```bash
npm --prefix asset-web ci
```

启动 HTTP 服务：

```bash
cargo run -p asset-http --bin asset-http
```

默认监听 `http://127.0.0.1:8080`。`--addr` 修改监听地址，`--config` 指定配置文件，
`asset-http --help` 查看完整参数。在另一个终端启动 Web 页面：

```bash
cd asset-web
npm run dev
```

打开 `http://127.0.0.1:5173`。浏览器直接进入全局资源工作区：浏览目录、上传资源、编辑文本
内容、下载或删除资源。启动会自动创建所需本地数据，无需账户或权限配置。

## 本地管理资源

Resource 与 Directory 操作始终经过 Core 应用服务。`asset` CLI 在此之上提供本地管理能力：

```bash
cargo run -p asset-cli --bin asset -- config --check
cargo run -p asset-cli --bin asset -- system --scan-resource
```

`asset config` 校验并输出生效配置；`asset system --scan-resource` 重新遍历 Blob 存储、重算全部
SHA-256，并把结果协调回资源数据库。完整命令说明见
[asset-cli/README.md](asset-cli/README.md)。

## 配置

可执行文件依次读取 `--config <PATH>`、`./config.toml`、内置默认值。
[config.example.toml](config.example.toml) 说明了全部配置项：

- `[database]` 选择数据库后端（SQLite）及连接池大小。
- `[resource_edit]` 限制交互式文本编辑；`[idempotency]` 控制请求租约。
- `[blob]` 选择 Blob 后端（本地文件系统）及其根目录；`[blob.local.sync]` 保持数据库与文件系统
  中的直接变更保持一致。

## 仓库结构

- `asset-core`：领域、端口与应用服务。
- `asset-infra`：SQLite 仓储、OpenDAL 存储与文件系统适配器。
- `asset-runtime`：可复用运行时组装与后台任务所有权。
- `asset-http`：Axum 传输、DTO、OpenAPI 与 HTTP 可执行文件。
- `asset-cli`：管理命令与 CLI 可执行文件。
- `asset-web`：React 浏览器应用。

各模块目录内文档描述其自身的架构、契约与测试命令。

## 开发

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
npm --prefix asset-web run check
```

生成的 OpenAPI 文档位于 `http://127.0.0.1:8080/api-docs/openapi.json`；`asset-web` 通过
`npm run generate:api` 从它重新生成传输类型（见
[asset-http/README.md](asset-http/README.md)）。
