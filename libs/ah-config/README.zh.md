# ah-config

[English](README.md)

提供类型化 TOML 配置、独立 space 分组、经过校验的配置快照，以及用于应用自有配置类型的派生宏。

## 使用指南

在使用方的 `Cargo.toml` 中添加依赖：

```toml
[dependencies]
ah-config = "0.1.0"
```

| Feature | 提供的能力 |
| --- | --- |
| `derive`（默认） | `#[derive(ah_config::Config)]` 和 `#[derive(ah_config::ConfigValue)]` |

设置 `default-features = false` 可以使用不含派生宏的加载 API。`Registry`、`LoadedConfig`、`ConfigError` 和 `ConfigErrorKind` 始终可用。

`Config` 声明配置节，必须指定 key；`ConfigValue` 声明嵌套结构体或枚举，不注册配置节。两者自动生成 Serde 的 `Serialize` 和 `Deserialize`，不要再次派生这些 trait。`Clone`、`Debug` 等标准派生能力自行选择。

| `#[config(...)]` 选项 | 声明的行为 |
| --- | --- |
| `key = "http"` | 必填 TOML 配置节路径，支持 `service.http` 等点分路径 |
| `spaces = ["default", "plugins"]` | 所属 space，省略时仅属于 `default` |
| `default = 表达式` | 类型上生成 `Default`；字段上补充缺失值。函数使用 `default = factory()` |
| `default` | 字段使用 `Default`；结构体根据字段声明生成 `Default` |
| `default = Self::Variant` | 在枚举类型上选择默认变体，并根据其字段声明生成 `Default` |
| `validate = 校验函数` | 配置节校验函数，接收 `&T` 并返回 `Result<(), E>` |
| `tag = "backend"` | 带字段枚举的变体选择字段，不支持用于结构体 |
| `rename = "名称"` | 单个字段或变体的外部名称 |
| `deny_unknown_fields` | 按需启用严格字段检查 |

字段和枚举变体固定使用 `snake_case`，例如 `LocalFile` 对应 `local_file`。标准符号（如 `MiB`）或特殊名称通过单项 `rename` 指定。显式声明的配置节 key、tag 名称和字符串值保持原样。

`Config` 和 `ConfigValue` 的所有类型级 `default` 声明都会生成 `Default`。结构体上的裸 `default` 根据字段默认值构造实例；`default = 表达式` 返回该表达式的结果，包括函数调用。枚举上的 `default = Self::Variant` 根据字段默认值构造所选变体。自动构造字段时，需要显式默认值或 `Option<T>`。声明了类型级默认值后，不要再手写或派生 `Default`；复杂构造逻辑放在工厂函数中。工厂函数不得递归调用宏生成的 `Default`。

`Config` 将该生成实现用于配置节的加载默认值。没有类型级默认声明时，允许手写或派生 `Default`，但不会自动将其用作加载默认值。嵌套类型实现了 `Default`，本身不会为缺失的父级字段补值；通过 `#[config(default)]` 明确使用它，或提供显式默认表达式。直接调用 `Default::default()` 只构造值，不执行配置校验。

字段缺失时使用可用的继承默认值或字段默认值；`Option<T>` 得到 `None`，没有可用默认值的字段仍然必填。字段类型实现了 `Default`，不会自动使该字段变成可选。部分填写的嵌套结构体使用其字段默认值，不会隐式调用类型的 `Default`；从父级继承的默认值仍然递归合并。

没有类型默认值的结构体配置节，可以通过字段默认值和可选字段构造，仍缺必填字段时则报错。结构体的类型默认值与文件字段递归合并，数组和任意嵌套层级中显式提供的枚举值整体替换。省略枚举时保留继承的默认值；显式提供时必须填写选择字段及必填字段，所选变体自身的字段默认值仍然生效，选择同一变体时也遵循此规则。显式填写的非法值不会回退到默认值。同一位置只能声明一个 `default`。函数调用遵循普通 Rust 表达式语法，需要括号；不会隐式调用裸函数名。校验错误须可转换为 `Box<dyn Error + Send + Sync>`。

使用 `Registry::for_space(name)` 选择 space，通过 `load_required(path)` 或 `load_optional(path)` 加载 TOML 文件，再通过 `get::<T>()` 取得共享的 `Arc<T>`。显式指定 spaces 会替换默认分组，space 名称不会为 TOML 路径添加前缀。所选配置全部加载并通过校验后，才返回配置快照。

直接向任一加载方法传入 `"config/config.toml"` 这样的相对文件路径即可。通过 Cargo 运行或测试时，自动以 package 根目录为起点，Release 运行同样生效。直接启动程序且没有继承 Cargo 运行环境时，以可执行文件所在目录为起点。两者均不受工作目录影响。绝对文件路径直接使用。需要自定义位置时，在加载前调用 `with_base_dir(绝对目录)` 覆盖自动选择的起点。使用相对文件路径加载时，相对或空的基础目录会报错。配置值内部的路径不会被改写。

可选文件允许缺失，其他加载失败仍然报错。每次加载产生独立快照。文件变化不会修改已有配置值，重新加载即可取得新值。默认值合并、校验及错误处理见下方案例。

通过 `ConfigError::kind()` 判断错误类别，通过 `path()`、`section()` 和 `space()` 定位问题。位置方法返回 `Option`：准备阶段的错误没有文件路径，文件级错误可能没有明确的注册配置节。`Error::source()` 保留底层诊断信息，`Display` 和 `Debug` 不输出配置值及底层原因。配置节的祖先节点不是表时，返回 `Deserialize`，而不是 `MissingSection`。

### 案例

建议按下表顺序阅读案例。每个案例围绕一个配置问题，文件开头包含运行方式；阅读 Rust 文件时，对照同目录、同文件名前缀的 TOML 文件。

前四个案例直接将校验融入配置声明，并提供由测试加载的非法配置文件：类型转换拒绝错误类型，校验函数检查包含默认值在内的最终配置。

| 案例 | 演示内容 |
| --- | --- |
| [基础入门](examples/basic.rs) | 声明 → 选择分组 → 加载 → 读取的流程、必填和可选字段、通过 `Config` 和 `ConfigValue` 定义嵌套类型，以及配置节校验 |
| [默认值](examples/defaults.rs) | 值表达式、函数调用和 `Default`、嵌套默认值和部分覆盖，以及拒绝非法值而不回退到默认值 |
| [枚举配置](examples/database.rs) | 变体选择字段、各变体的必填字段、枚举类型上的默认选项，以及所选变体的校验 |
| [Space 分组](examples/spaces.rs) | 在独立选择的 space 中声明私有配置和共享配置，仅校验所选声明 |
| [配置快照](examples/snapshots.rs) | 显式加载新快照，同时保留已有的共享配置值 |

## 维护手册

以下命令均在 ah-config 源码根目录执行，即本 README 和 `Cargo.toml` 所在的目录。

- `src/` 包含公开 API，`macros/src/` 实现派生宏。
- `examples/` 包含用法和行为测试；`tests/loading.rs`、`tests/spaces.rs` 分别检查加载规则和 space 隔离，`tests/errors.rs` 检查错误上下文与底层原因，`tests/defaults.rs` 检查默认值规则，`tests/nested_defaults.rs` 检查嵌套枚举替换，`tests/naming.rs` 检查命名和单项覆盖，`tests/paths.rs` 检查文件路径解析与工作目录无关性，`tests/runtime.rs` 检查没有配置声明时的加载行为。
- `tests/fixtures/` 包含用于集成检查的独立使用方项目。

### API 文档

[API 参考文档](https://docs.rs/ah-config/latest/ah_config/) 说明生成的实现、属性位置和类型限制。本地构建方式：

```sh
cargo doc --all-features --no-deps
```

生成的文档位于 `target/doc/ah_config/index.html`，使用浏览器打开即可。如果设置了 `CARGO_TARGET_DIR`，文档位于该目录下的 `doc/ah_config/index.html`。

### 检查

安装 Python 3.12+ 和带有 `rustfmt`、`clippy` 的 Rust 工具链，在完整仓库中执行统一检查：

```sh
python3 ci/check.py
```

该入口检查测试与案例、feature 组合、格式、Clippy 和文档，还会在 Debug 及 Release/LTO 下运行依赖改名与外部 crate 注册的独立使用方，核对预期编译失败的案例（包括默认声明冲突与重复实现 `Default`），并解包实际生成的 `.crate` 归档后编译。解包后的运行库通过临时本地 patch 使用解包后的宏包。整个过程不会发布包。

检查遇错即停，返回非零退出码，完整日志保存在 `target/ci-logs/native.log`；依赖已缓存时，可以设置 `CARGO_NET_OFFLINE=true`。执行与日志复用仓库的 `.github/checks`，与 ah-id 一致，无须调整 CI 工作流。

独立源码副本仍可直接使用 Cargo，例如 `cargo test --workspace --all-features --locked`。普通 `cargo test` 不会执行独立使用方检查或归档验证。

## 许可证

版权所有 © 2026 AzVs。本模块采用 [Apache License 2.0](./LICENSE) 许可证。
