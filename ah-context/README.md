# ah-context

自动登记类型，在应用启动时创建全局共享上下文。使用流程：声明注册 → `load()`
初始化 → `get::<T>()` 获取共享实例。

## 使用示例

`#[context()]` 在定义结构体时登记；`register!` 在模块顶层登记已有类型。
两种方式都默认使用 `Default`，也都支持 `init = path` 自定义初始化。

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

fn create_greeting() -> Result<Greeting, std::io::Error> {
    Ok(Greeting { text: "hello".into() })
}

fn main() -> Result<(), ah_context::ContextError> {
    ah_context::load()?;

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

类型本身仍是普通 Rust 类型；context 持有共享实例，每次获取返回同一实例的 `Arc<T>`。
共享内容需要修改时，使用锁或原子类型等内部可变性。

## 注意事项

- 类型须满足 `Send + Sync + 'static`，由编译器自动检查。`'static` 要求不借用短生命周期
  数据，不要求声明为 `static`；`String`、`Vec<String>` 可在运行时创建并满足这些约束。
- `#[context()]` 用于非泛型结构体；`register!` 支持具体类型，包括 `Container<u32>`
  等确定的泛型实例。同一类型只登记一次，否则 `load()` 返回重复注册错误。
- 默认初始化需要 `Default`；指定 `init` 后不要求 `Default`。`init` 是无参同步函数，
  返回 `Result<T, E>`，其中 `E: std::error::Error + Send + Sync + 'static`。
- 启动时先调用 `load()`，再使用 `get()`。未初始化或查询未登记类型时，`get()` 返回错误。
  重复或并发调用 `load()` 返回同一个已成功初始化的上下文。
- 初始化函数不能递归调用 `load()`，也不能从尚未发布的上下文获取其他内容；
  需要的依赖应提前准备好，不要依赖注册声明的顺序。
- 全部构造成功后才发布上下文。初始化返回错误时可以重试，但构造函数可能重跑，
  外部副作用不回滚；panic 不提供恢复机制。
- 注册声明在构建和链接时自动收集，初始化后集合固定，不支持运行时新增或移除类型。
