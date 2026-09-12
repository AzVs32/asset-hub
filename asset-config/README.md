# Asset Config

`asset-config` 是一个纯粹的配置工具模块，为 Asset Hub 的各个可执行程序提供统一的 TOML
加载和强类型配置分区注册。它不拥有任何业务、基础设施、终端或插件配置。

## 模块如何声明配置

需要配置的模块先添加依赖：

```toml
[dependencies]
asset-config.workspace = true
serde.workspace = true
```

以一个假想的 `asset-preview` 模块为例，在该模块的 `config.rs` 中定义配置类型：

```rust
use asset_config::ConfigSection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PreviewConfig {
    pub enabled: bool,
    pub max_items: usize,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_items: 100,
        }
    }
}

impl ConfigSection for PreviewConfig {
    const SECTION: &'static str = "plugins.preview";

    fn normalize(self) -> Result<Self, String> {
        if self.enabled && self.max_items == 0 {
            return Err("max_items must be greater than 0 when enabled".to_owned());
        }
        Ok(self)
    }
}
```

配置类型需要满足以下约定：

- `SECTION` 声明该类型拥有的 TOML 路径。
- `Default` 提供整个分区缺失时的默认值。
- `Deserialize` 和 `Serialize` 用于加载及输出 TOML。
- 建议使用 `#[serde(default, deny_unknown_fields)]`：允许字段缺失时使用默认值，同时拒绝
  本分区内拼错的字段。
- `normalize()` 由配置所属模块实现，用于路径归一化和跨字段语义校验。

对应的 TOML 为：

```toml
[plugins.preview]
enabled = true
max_items = 200
```

## 可执行程序如何注册和加载

编译期已经确定的模块使用链式 `with::<T>()`：

```rust
use asset_config::ConfigRegistry;
use asset_http::HttpConfig;
use asset_preview::PreviewConfig;
use asset_runtime::AssetConfig;

let config = ConfigRegistry::new()
    .with::<AssetConfig>()?
    .with::<HttpConfig>()?
    .with::<PreviewConfig>()?
    .load(config_path)?;
```

`load()` 只读取并解析一次共享 TOML 文档，然后依次反序列化、归一化和校验所有已注册分区。
显式路径必须存在；路径为 `None` 时统一尝试读取当前目录的 `config.toml`，文件不存在则按照空
文档加载所有已注册类型的默认值。默认文件名属于 `asset-config` 的内部约定，不作为常量暴露。

需要不同来源策略时，也可以调用 `load_file(path)`、`load_optional_file(path)`，或者使用
`load_str(source)` 从已有 TOML 字符串加载。

注册必须发生在加载之前。分区不能重复，也不能存在父子所有权重叠。例如可以同时注册
`plugins.preview` 和 `plugins.search`，但不能同时注册 `plugins` 和 `plugins.preview`。

## 模块如何取得自己的配置

通过配置类型提取结果，不需要再次使用字符串路径：

```rust
let asset_config = config.section::<AssetConfig>()?.clone();
let http_config = config.section::<HttpConfig>()?.clone();
let preview_config = config.section::<PreviewConfig>()?.clone();
```

`section::<T>()` 返回 `&T`。当后续组件需要取得配置所有权时再调用 `clone()`；只读使用时不必克隆。

## 职责边界

`asset-config` 只公开 `ConfigSection`、`ConfigRegistry`、`LoadedConfig`、`ConfigError` 和默认
配置文件名。`AssetConfig` 由 `asset-runtime` 拥有，`HttpConfig` 由 `asset-http` 拥有，未来
插件配置由各插件拥有；本模块不依赖这些具体配置类型。

源码按公开概念组织：`section.rs` 定义分区约定，`registry.rs` 负责注册和加载，
`loaded.rs` 负责强类型提取和输出。`registration.rs` 与 `document.rs` 是类型擦除和 TOML
路径处理的内部实现。

## 未注册分区和配置输出

未注册分区不会被当前可执行程序反序列化或进行语义校验，但其值会保留在共享文档中。这使
CLI、HTTP 服务和插件可以共用同一个配置文件，而不必互相依赖配置类型。

已注册分区仍会严格校验自己的完整子树。可以通过以下接口输出填充默认值并完成归一化后的
配置，同时保留未注册分区：

```rust
let toml = config.to_toml_string()?;
```

## 验证

```bash
cargo test -p asset-config
```
