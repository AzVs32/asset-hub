# ah-context

按类型声明、加载和共享应用上下文，使用流程为：声明 → `load` → `get`。

## 使用示例

`#[context()]` 声明结构体并自动登记；`register!` 在模块顶层登记已有类型。
两种方式默认使用 `Default`，也都支持 `init = path` 自定义初始化。

添加 `ah-context` 依赖后，以下代码可以直接作为 `main.rs`：

```rust
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[ah_context::context()]
#[derive(Default)]
struct Counter {
    value: AtomicUsize,
}

struct Greeting {
    text: String,
}

ah_context::register!(Greeting, init = create_greeting);

fn create_greeting() -> Result<Greeting, &'static str> {
    Ok(Greeting { text: "hello".into() })
}

fn main() -> Result<(), ah_context::ContextError> {
    let context = ah_context::load()?;
    assert!(std::ptr::eq(context, ah_context::load()?));

    let first = ah_context::get::<Counter>()?;
    let second = ah_context::get::<Counter>()?;
    first.value.fetch_add(1, Ordering::Relaxed);
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(second.value.load(Ordering::Relaxed), 1);

    let greeting = ah_context::get::<Greeting>()?;
    assert_eq!(greeting.text, "hello");
    Ok(())
}
```

类型本身仍是普通 Rust 类型；同一上下文的 `get::<T>()` 返回同一实例的 `Arc<T>`。
共享内容需要修改时，使用锁或原子类型等内部可变性。

## 注意事项

- 使用流程是声明 → `load` → `get`。`load()` 收集自动登记项，通过
  `ContextBuilder::build()` 构造全部实例，再发布全局上下文。
- **加载生命周期不同：ah-context 的 `load()` 是进程级全局幂等的，成功后重复或并发调用
  返回同一个 `Context`；asset-config 的 `load(path)` 每次独立读取文件并返回新的
  `LoadedConfig`，即使路径相同也重新加载。** ah-context 成功加载后不会重新运行构造函数。
- 类型须满足 `Send + Sync + 'static`。`'static` 要求不借用短生命周期数据，不要求声明
  为 `static`；`String`、`Vec<String>` 可在运行时创建并满足这些约束。
- `#[context()]` 用于非泛型结构体；`register!` 支持已有类型及 `Container<u32>` 等
  具体泛型实例。登记阶段检查全部类型是否重复，任何构造函数运行前就会返回重复错误。
- 默认初始化需要 `Default`；指定 `init` 后不要求 `Default`。`init` 是无参同步函数，
  返回 `Result<T, E>`，其中 `E: Into<Box<dyn std::error::Error + Send + Sync>>`。
  `&str`、`String`、满足 `Send + Sync + 'static` 的错误和已装箱错误均可使用；
  `Initialization { type_name, source }` 保留原始错误链。该错误的 `Display` 只描述
  初始化失败的类型，具体原因通过 `std::error::Error::source()` 或错误链报告器展示，
  避免同一原因重复打印。
- 全局 `get()` 必须在 `load()` 或 `load_with(builder)` 成功后调用，否则返回
  `NotInitialized`。查询未登记类型返回 `NotRegistered { type_name }`。
  `ContextError` 标记为 `#[non_exhaustive]`，匹配时
  需要保留兜底分支。
- `load_with(builder)` 构造并发布传入的 Builder，不额外收集自动登记项。需要包含
  自动声明时使用 `ContextBuilder::from_auto()`。它与 `load()` 共用初始化锁；如果
  全局上下文已经发布，返回 `AlreadyInitialized`，被拒绝的 Builder 不执行构造函数。
  成功后调用 `load()` 会复用该上下文。
- 初始化函数不能递归调用全局 `load()` 或 `load_with()`，也不能从尚未发布的上下文
  获取其他内容；需要的依赖应提前准备好，不要依赖自动登记声明的顺序。
- 全部构造成功后才发布上下文。初始化返回错误时可以重试，但构造函数可能重跑，
  外部副作用不回滚；panic 不提供恢复机制。
- 自动登记声明在构建和链接时收集，已构建的 `Context` 集合固定。测试或按需组装可用
  `ContextBuilder` 创建独立上下文。

### 独立上下文

`ContextBuilder` 从空集合开始；`register::<T>()` 使用 `Default`，
`register_with(init)` 接受返回 `Result<T, E>` 的无参同步闭包，可以捕获拥有所有权的
依赖，闭包须满足 `FnOnce + Send + 'static`，错误类型约束与自动初始化一致：

```rust
use ah_context::ContextBuilder;

#[derive(Default)]
struct Counter(std::sync::atomic::AtomicUsize);
struct Settings(String);

fn main() -> Result<(), ah_context::ContextError> {
    let label = String::from("test");
    let mut builder = ContextBuilder::new();
    builder.register::<Counter>()?;
    builder.register_with(move || Ok::<_, &'static str>(Settings(label)))?;
    let context = builder.build()?;
    assert_eq!(context.get::<Settings>()?.0, "test");
    Ok(())
}
```

登记时立即检查重复类型，尚不执行构造函数。`build()` 消费 Builder，按登记顺序构造
实例，全部成功后返回 `Context`。失败时丢弃已经构造的实例；重试需要重新创建 Builder，
外部副作用不回滚。通过 `build()` 创建的上下文相互独立，不会发布或修改全局上下文。

`ContextBuilder::from_auto()` 收集属性宏和 `register!` 的登记项，可在 `build()` 前
继续登记其他类型，也可构建独立实例；它同样先检查全部登记项，再开始构造。

### 配置驱动的全局上下文

同时添加 `asset-config` 依赖后，可以先加载配置，再通过闭包将配置注入服务，最后
用 `load_with(builder)` 发布全局上下文。以下代码可以直接作为 `main.rs`：

```rust,standalone_crate
use std::convert::Infallible;
use std::error::Error;
use std::sync::Arc;

#[asset_config::config(key = "http")]
struct HttpConfig {
    port: u16,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self { port: 8080 }
    }
}

struct HttpClient(Arc<HttpConfig>);

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let configs = asset_config::load("config.toml")?;
    let http = configs.get::<HttpConfig>()?;

    let mut builder = ah_context::ContextBuilder::from_auto()?;
    builder.register_with(move || Ok::<_, Infallible>(HttpClient(http)))?;
    let context = ah_context::load_with(builder)?;

    let client = ah_context::get::<HttpClient>()?;
    assert!(Arc::ptr_eq(&client.0, &configs.get::<HttpConfig>()?));
    assert!(std::ptr::eq(context, ah_context::load()?));
    println!("HTTP port: {}", client.0.port);
    Ok(())
}
```

`load_with` 消费 Builder。构造失败时不发布上下文，可以重新组装后重试；已发布的
全局上下文不会被替换。仅为测试组装服务时，直接使用 `builder.build()`。
