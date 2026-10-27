# ah-config

[English](README.md)

提供类型化 TOML 配置、独立 space 分组、经过校验的配置快照，以及用于应用自有配置类型的派生宏。

## 使用指南

在使用方的 `Cargo.toml` 中添加依赖：

```toml
[dependencies]
ah-config = "0.2.0"
```

### 快速开始

在应用的 package 目录中创建 `config/config.toml`：

```toml
[http]
port = 9000
```

在 `src/main.rs` 中声明、加载并读取配置：

```rust
use ah_config::Registry;

#[derive(ah_config::Config)]
#[config(key = "http", deny_unknown_fields)]
struct HttpConfig {
    #[config(default = 8080)]
    port: u16,
}

fn main() -> Result<(), ah_config::ConfigError> {
    let config = Registry::for_space("default")?
        .load_optional("config/config.toml")?;
    for warning in config.warnings() {
        eprintln!("warning: {warning}");
    }
    let http = config.get::<HttpConfig>()?;
    println!("HTTP port: {}", http.port);
    Ok(())
}
```

执行 `cargo run`，输出 `HTTP port: 9000`。文件不存在时，加载自动使用声明的默认值，
并由应用输出警告；省略 `port` 时使用 `8080`。`port = "wrong"` 这样的非法值会报错，
不会回退到默认值。全部选中的配置节
完成解析和校验后，加载才会返回。通过 Cargo 运行时，相对文件路径以 package 目录为起点；
直接启动程序时以可执行文件所在目录为起点。需要明确指定时，使用
`with_base_dir(绝对目录)`。

### 查看已注册配置

```rust
for section in ah_config::Registry::declarations() {
    println!("{}: {} in {:?}", section.key(), section.type_name(), section.spaces());
}
println!("Available spaces: {:?}", ah_config::Registry::spaces());

let registry = ah_config::Registry::for_space("default")?;
for section in registry.sections() {
    println!("Selected: {}", section.key());
}
```

`Registry::spaces()` 返回已链接的 space 名称，排序并去重。
`Registry::declarations()` 返回全部已链接配置节，按完整 key、Rust 类型名和分组归属排序，
保留冲突声明以便排查。这两个全局查询不读取文件、不执行默认值工厂或校验函数，
即使选择某个 space 会失败也能查询。Registry 的 `sections()` 列出已选声明；
加载后的快照也通过 `sections()` 保留这份列表。
仅实际链接进当前程序的 crate 会出现在结果中。类型名用于诊断，不作为持久化标识。
`UnknownSpace` 的打印结果和 `available_spaces()` 方法会给出可用分组。

### 文件缺失与警告

使用 `load_optional(path)`：文件存在时读取，读取返回 `NotFound` 时自动使用声明的默认值。
默认值仍须通过校验，没有默认值的必填字段仍会报错。成功回退后，快照记录
`ConfigWarning::MissingFile { path }`，其中路径是解析后的绝对路径。
通过 `LoadedConfig::warnings()` 读取，并按快速开始示例接入应用自己的日志。
库本身不直接打印警告。已存在的空文件不会产生文件缺失警告；即使稍后创建了文件，
旧快照仍保留当时的警告。

如果要求文件必须存在，继续使用 `load_required(path)`。
两种入口都不会在权限、语法、类型或校验失败时回退。

### 声明与默认值速查

| 声明方式 | 归属与读取方式 |
| --- | --- |
| `Config` 配合 `key = "http"` | 独立根配置节，通过 `get::<HttpConfig>()` 读取 |
| `Config` 配合 `parent = VfsConfig, key = "wasm"` | 独立子配置节，单独读取，无需增加父类型字段 |
| `ConfigValue` | 由其他配置类型的字段拥有，通过该字段读取 |

`get::<T>()` 要求实现 `Config` 自动生成的公开 `ConfigSection` trait。
普通类型和 `ConfigValue` 类型会在编译期报错。配置节类型合法但未属于当前选中的
space 时，返回 `NotRegistered`，并包含其 key 和配置快照的位置。
泛型辅助函数可使用 `T: ah_config::ConfigSection`，`T::KEY` 是其完整 TOML 路径。
父子关系与分组归属相互独立：子配置不会继承父类型的 spaces，也不会自动选中父配置。

| 默认值声明 | 输入缺失时的行为 |
| --- | --- |
| 字段 `#[config(default = 表达式)]` | 使用该表达式补充缺失字段 |
| 字段 `#[config(default)]` | 使用字段类型的 `Default` |
| `Option<T>` 字段 | 没有继承默认值时得到 `None` |
| 结构体 `#[config(default)]` | 根据字段默认声明生成 `Default`，并用作配置节默认值 |
| 类型 `#[config(default = factory())]` | 通过工厂生成 `Default`，并用作配置节默认值 |
| 普通 `#[derive(Default)]` 或手写 `Default` | 构造 Rust 值，不会自动成为配置节加载默认值 |
| 直接调用 `Default::default()` | 仅构造值，不执行配置校验 |

嵌套类型声明了默认值，不会自动补充缺失的父字段；需要在该字段上使用
`#[config(default)]` 明确启用。对于 `ConfigValue`，类型级默认声明仅生成供父字段使用的
`Default`，不注册配置节。类型级 `#[config(default...)]` 已生成 `Default` 时，不要再次
派生或手写实现。详细合并规则见下文。

### 声明选项

| Feature | 提供的能力 |
| --- | --- |
| `derive`（默认） | `#[derive(ah_config::Config)]` 和 `#[derive(ah_config::ConfigValue)]` |

设置 `default-features = false` 可以使用不含派生宏的加载 API。`Registry`、`LoadedConfig`、`ConfigSection`、`SectionInfo`、`ConfigWarning`、`ConfigError` 和 `ConfigErrorKind` 始终可用。

`Config` 声明配置节，必须指定 key；`ConfigValue` 声明嵌套结构体或枚举，不注册配置节。两者自动生成 Serde 的 `Serialize` 和 `Deserialize`，不要再次派生这些 trait。`Clone`、`Debug` 等标准派生能力自行选择。

| `#[config(...)]` 选项 | 声明的行为 |
| --- | --- |
| `key = "http"` | 必填的单个 TOML 路径段；子配置节使用 `parent` 声明 |
| `parent = VfsConfig` | 注册到父类型之下，在编译期检查父类型是否允许扩展 |
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

通过 `ConfigError::kind()` 判断错误类别，通过 `path()`、`section()`、
`field_path()` 和 `space()` 定位问题。`Display` 和 `Debug` 显示可用的位置元数据，
不输出配置值及底层原因。例如，非法的 HTTP 端口可输出：

```text
configuration section has an invalid type or field [file: "/app/config/config.toml"] [section: "http"] [field: "http.port"] [space: "default"]
```

解析器提供字段位置时，`field_path()` 包含完整 key 和数组下标，例如
`service.http.limits.0.memory_limit_mb`。位置方法返回 `Option`：准备阶段没有文件路径，
语法错误可能没有配置节位置，配置节校验函数也不指定具体字段。原始解析、I/O 和校验错误
仍可通过 `Error::source()` 获取；显式打印底层错误链可能包含输入值或自定义消息。
祖先节点不是表时，返回 `Deserialize`，而不是 `MissingSection`。

### 独立子配置节

模块可以追加 `vfs.wasm`，无需修改原本的 `VfsConfig`。子配置显式声明父类型，
父类型可以由其他 crate 导出：

```rust
#[derive(ah_config::Config)]
#[config(key = "vfs")]
pub struct VfsConfig {
    enabled: bool,
}

#[derive(ah_config::Config)]
#[config(parent = VfsConfig, key = "wasm")]
pub struct WasmConfig {
    memory_limit_mb: u64,
}
```

```toml
[vfs]
enabled = true

[vfs.wasm]
memory_limit_mb = 128
```

加载同一个 space 后，分别通过 `get::<VfsConfig>()` 和 `get::<WasmConfig>()` 读取。
同级配置可声明 `parent = VfsConfig, key = "rpc"`；更深层的子配置声明其直接父类型。
原本的 Rust 结构体无需包含 `wasm` 或 `rpc` 字段。

只有派生了 `Config` 且未启用 `deny_unknown_fields` 的具名字段结构体允许外部子配置。
严格父配置、`ConfigValue` 父类型、枚举父类型，以及与父类型已有字段重名的子 key
（包括重命名后的字段名），都会导致编译失败。每个 key 必须是非空的单个路径段，
仅包含 ASCII 字母、数字、下划线或连字符。不再允许 `key = "vfs.wasm"` 这样的裸点分 key；
已有声明应迁移为显式父类型关系，避免绕过扩展检查。完全相同的注册路径仍在选择 space 时
返回 `KeyConflict`，包括在不同 crate 中声明的重复路径。

子配置也可以通过新的显式 `parent` 声明成为父配置。层级深度不会自动启用严格检查：
只有显式声明 `deny_unknown_fields` 才会关闭具名配置节的继续扩展能力。

每个配置节保留自己的默认值和校验函数。Space 选择保持独立：省略 `spaces` 仍仅属于
`default`，子配置不会继承父类型的分组，也不会自动加载父配置。需要同时加载时，
将二者声明在同一个 space 中。允许扩展的父配置会忽略未知字段，包括未注册的子表；
扩展许可并不意味着能在编译期校验任意 TOML 文件中的 key。

`ConfigValue` 用于另一种场景：如果 `VfsConfig` 显式包含 `wasm: WasmConfig` 字段，
嵌套类型应派生 `ConfigValue`，通过该字段读取。它不独立注册，调用
`get::<WasmConfig>()` 会编译失败，同一字段也不能再作为外部子配置独立注册。

### 案例

建议按下表顺序阅读案例。每个案例围绕一个配置问题，文件开头包含运行方式；阅读 Rust 文件时，对照同目录、同文件名前缀的 TOML 文件。

前四个案例直接将校验融入配置声明，并提供由测试加载的非法配置文件：类型转换拒绝错误类型，校验函数检查包含默认值在内的最终配置。

| 案例 | 演示内容 |
| --- | --- |
| [基础入门](examples/basic.rs) | 声明 → 选择分组 → 加载 → 读取的流程、必填和可选字段、通过 `Config` 和 `ConfigValue` 定义嵌套类型，以及配置节校验 |
| [默认值](examples/defaults.rs) | 值表达式、函数调用和 `Default`、嵌套默认值和部分覆盖，以及拒绝非法值而不回退到默认值 |
| [枚举配置](examples/database.rs) | 变体选择字段、各变体的必填字段、枚举类型上的默认选项，以及所选变体的校验 |
| [Space 分组](examples/spaces.rs) | 在独立选择的 space 中声明私有配置和共享配置，仅校验所选声明 |
| [独立子配置](examples/extensions.rs) | 独立的 `vfs.rpc`、`vfs.wasm` 配置节、内部嵌套值以及子配置校验 |
| [配置快照](examples/snapshots.rs) | 显式加载新快照，同时保留已有的共享配置值 |

## 维护手册

以下命令均在 ah-config 源码根目录执行，即本 README 和 `Cargo.toml` 所在的目录。

- `src/` 包含公开 API，`macros/src/` 实现派生宏。
- `examples/` 包含用法和行为测试；`tests/loading.rs`、`tests/spaces.rs` 检查加载规则、文件缺失警告和 space 隔离，`tests/discovery.rs` 检查无副作用的声明查询和已选元数据，`tests/errors.rs` 检查错误上下文与底层原因，`tests/defaults.rs` 检查默认值规则，`tests/nested_defaults.rs` 检查嵌套枚举替换，`tests/naming.rs` 检查命名和单项覆盖，`tests/extensions.rs` 检查父子默认值、多层子配置与分组隔离，`tests/paths.rs` 检查文件路径解析与工作目录无关性，`tests/runtime.rs` 检查没有配置声明时的加载行为。
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

该入口检查测试与案例、feature 组合、格式、Clippy 和文档，还会在 Debug 及 Release/LTO 下运行依赖改名与外部 crate 注册的独立使用方，核对预期编译失败的案例（包括默认声明冲突、重复实现 `Default` 和非法父子配置声明），并解包实际生成的 `.crate` 归档后编译。解包后的运行库通过临时本地 patch 使用解包后的宏包。整个过程不会发布包。

检查遇错即停，返回非零退出码，完整日志保存在 `target/ci-logs/native.log`；依赖已缓存时，可以设置 `CARGO_NET_OFFLINE=true`。执行与日志复用仓库的 `.github/checks`，与 ah-id 一致，无须调整 CI 工作流。

独立源码副本仍可直接使用 Cargo，例如 `cargo test --workspace --all-features --locked`。普通 `cargo test` 不会执行独立使用方检查或归档验证。

## 许可证

版权所有 © 2026 AzVs。本模块采用 [Apache License 2.0](./LICENSE) 许可证。
