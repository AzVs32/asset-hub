# asset-config

按类型声明、加载和共享应用配置，使用流程为：声明 → `load` → `get`。

## 使用示例

添加 `asset-config` 依赖后，以下代码可以直接作为 `main.rs`：

```rust
use std::sync::Arc;

#[asset_config::config(key = "http", validate = validate_http)]
struct HttpConfig {
    host: String,
    port: u16,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 8080,
        }
    }
}

fn validate_http(config: &HttpConfig) -> Result<(), &'static str> {
    if config.port == 0 {
        Err("port must be positive")
    } else {
        Ok(())
    }
}

fn main() -> Result<(), asset_config::ConfigError> {
    let configs = asset_config::load("config.toml")?;
    let first = configs.get::<HttpConfig>()?;
    let second = configs.get::<HttpConfig>()?;
    assert!(Arc::ptr_eq(&first, &second));
    println!("{}:{}", first.host, first.port);

    let reloaded = asset_config::load("config.toml")?;
    assert!(!Arc::ptr_eq(&first, &reloaded.get::<HttpConfig>()?));
    Ok(())
}
```

文件不存在时使用 `Default`。可以在 `config.toml` 中覆盖部分字段：

```toml
[http]
port = 9090
```

## 注意事项

- 使用流程是声明 → `load` → `get`。`#[config]` 声明配置节并默认自动登记非泛型类型，
  `load(path)` 加载全部自动登记项，随后通过返回的 `LoadedConfig` 获取 `Arc<T>`。
- **加载生命周期不同：ah-context 的 `load()` 是进程级全局幂等的，成功后重复或并发调用
  返回同一个 `Context`；asset-config 的 `load(path)` 每次独立读取文件并返回新的
  `LoadedConfig`，即使路径相同也重新加载。** 修改文件不会改变已加载的配置实例。
- 类型须满足 `Default + Serialize + DeserializeOwned + Send + Sync + 'static`。
  `#[config]` 补齐所需的 Serde 派生，并应放在 `#[derive(...)]` 前面。`cfg` 和嵌套
  `cfg_attr` 的条件编译部分会传播到生成的实现和自动登记项。
- 配置文件覆盖默认值，缺失的字段保留默认值。每个已登记类型在加载时反序列化并校验
  一次，全部成功后才返回结果；`get::<T>()` 只克隆已校验实例的 `Arc`，不再解析或校验。
  返回的 `Arc<T>` 可以在 `LoadedConfig` 被释放后继续使用。
- `key` 必须是由点分隔的非空路径段，每段只含 ASCII 字母、数字、`_` 或 `-`。
  键区分大小写；相同键或父子路径冲突会在登记阶段被拒绝。
- `validate = path` 指定接受 `&T`、返回 `Result<(), E>` 的校验函数，其中
  `E: Into<Box<dyn std::error::Error + Send + Sync>>`。`&str`、`String`、满足
  `Send + Sync + 'static` 的错误和已装箱错误均可使用；`Validation { key, source }`
  保留原始错误链。
- 查询未登记的具体类型时返回 `NotRegistered { type_name, key }`。`ConfigError` 标记为
  `#[non_exhaustive]`，匹配时需要保留兜底分支。

### 具体泛型实例

泛型定义只生成 `ConfigSection` 实现，在模块顶层用 `register!` 登记具体实例，流程
仍为声明 → `load` → `get`：

```rust
#[asset_config::config(key = "limits")]
#[derive(Default)]
struct GenericConfig<T> {
    value: T,
}

asset_config::register!(GenericConfig<u32>);

fn main() -> Result<(), asset_config::ConfigError> {
    let configs = asset_config::load("config.toml")?;
    assert_eq!(configs.get::<GenericConfig<u32>>()?.value, 0);
    Ok(())
}
```

`register!` 也接受已有的非泛型 `ConfigSection`，使用其 `KEY`，不修改类型定义。
同一泛型定义的具体实例使用相同键，自动加载集合中应选择一个实例。

### 按需加载

`auto = false` 将非泛型配置留给测试或按需组装。`Registry` 从空集合开始，可以选择
非泛型类型或具体泛型实例；它的 `load(path)` 只加载当前集合，并消费 Registry：

```rust
#[asset_config::config(key = "test", auto = false)]
#[derive(Default)]
struct TestConfig {
    enabled: bool,
}

fn main() -> Result<(), asset_config::ConfigError> {
    let mut registry = asset_config::Registry::default();
    registry.register::<TestConfig>()?;
    let configs = registry.load("test-config.toml")?;
    assert!(!configs.get::<TestConfig>()?.enabled);
    Ok(())
}
```
