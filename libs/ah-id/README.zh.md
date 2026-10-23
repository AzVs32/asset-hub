# ah-id

[English](README.md)

提供强类型 ID、经过校验的 UUID v7、ULID、NanoID 值类型，以及用于自定义值类型的派生宏。

## 使用指南

在使用方的 `Cargo.toml` 中添加依赖，并启用需要的值类型：

```toml
[dependencies]
ah-id = { version = "0.1.0", features = ["nanoid"] }
```

| Feature | 提供的能力 |
| --- | --- |
| `derive`（默认） | `#[derive(ah_id::Id)]` |
| `uuid_v7` | `UuidV7` |
| `ulid` | `Ulid` |
| `nanoid` | `NanoId` |

设置 `default-features = false` 可以单独使用内置值类型。`Generate` 和 `InvalidIdError` 始终可用。

派生宏要求结构体只有一个私有元组字段，并且至少指定一个选项：

| `#[id(...)]` 选项 | 生成的接口 |
| --- | --- |
| `generate` | `new()` 和 `ah_id::Generate`，委托内部值生成 |
| `display` | `Display`，委托内部值显示 |
| `parse` | `FromStr`、`TryFrom<&str>`、`TryFrom<String>`，保留内部值的解析错误 |
| `as_ref` | `AsRef<Value>` |
| `deref` | 只读 `Deref<Target = Value>` |

选项互相独立。`Clone`、`Copy`、`Eq`、`Hash` 等标准派生能力自行选择。宏不会生成可变访问或 `Default`。

自动生成的解析和生成接口会直接包装合法的内部值，不会调用手写的 `TryFrom<Value>`。ID 的额外约束应放在内部值类型中，或者手动实现相应输入入口。文本格式由内部值的 `Display` 和 `FromStr` 约定。

### 案例

各案例文件中包含说明和运行方式。

| 案例 | 演示内容 |
| --- | --- |
| [NanoID](examples/nanoid.rs) | 值类型和强类型 ID 的生成、恢复 |
| [UUID v7](examples/uuid_v7.rs) | UUID v7 值类型和挂载 ID |
| [ULID](examples/ulid.rs) | ULID 值类型和事件 ID |
| [向量](examples/vector.rs) | 包装已有类型、校验输入、访问字段与方法 |
| [订单行](examples/order_line.rs) | 使用 NanoID 订单 ID 和行号组成联合主键 |

## 维护手册

以下命令均在 ah-id 源码根目录执行，即本 README 和 `Cargo.toml` 所在的目录。

维护工具需要从源码检出中使用，发布包不包含完整的维护工具。

- `src/` 包含公开 API，`macros/src/` 实现派生宏。
- `examples/` 包含用法和行为测试；`tests/derive.rs`、`tests/values.rs` 分别检查宏行为和值类型校验。
- `tests/fixtures/` 包含用于集成检查的独立使用方项目。

### API 文档

```sh
cargo doc --all-features --no-deps
```

生成的文档位于 `target/doc/ah_id/index.html`，使用浏览器打开即可。如果设置了 `CARGO_TARGET_DIR`，文档位于该目录下的 `doc/ah_id/index.html`。

### 检查

安装带有 `rustfmt`、`clippy` 的 Rust 工具链，以及 Python 3.12 或更新版本。完整检查脚本需要源码检出上级目录中的共享工具 `.github/checks/`：

```sh
python3 ci/check.py
```

检查范围包括格式、feature 组合、案例、依赖改名、Clippy、API 文档和打包产物，不会发布包。日志位于 `target/ci-logs/`，非零退出码表示失败或中断。

添加 `--docker` 检查本地全部四个 Linux 环境，也可以通过 `--docker arch` 等方式指定一个。Docker 检查需要访问 Docker 服务，并联网拉取镜像和安装依赖。GitHub CI 检查 Windows 和 macOS。脚本定义本包的检查内容，`.github/checks/` 提供共享执行与日志能力。

如果源码副本中没有共享工具，可以直接使用 Cargo 检查：

```sh
cargo test --workspace --all-features --locked
```

## 许可证

版权所有 © 2026 AzVs。本模块采用 [Apache License 2.0](LICENSE) 许可证。
