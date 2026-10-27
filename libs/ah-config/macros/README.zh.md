# ah-config-macros

为 `ah-config` 提供配置声明和值类型派生宏。使用方依赖 `ah-config`，
通过其重导出的 `Config` 和 `ConfigValue` 使用宏。

`Config` 注册配置节；`ConfigValue` 描述嵌套值，不进行注册。
两者自动生成 Serde 的 `Serialize` 和 `Deserialize`，不要再次派生这些 trait。
通过 `#[config(...)]` 声明字段默认值、枚举类型上的默认选项、
命名规则、枚举选择字段和校验。`default` 接受值表达式或调用表达式，例如 `default = factory()`。
枚举上的 `default = Self::Variant` 根据字段默认值构造指定变体。
两种宏的所有类型级默认声明都会生成 `Default`，不要再手写或派生它。
没有类型级默认声明时，允许手写或派生 `Default`，但不会隐式为配置节
或未标记的父级字段提供默认值。调用 `Default::default()` 不执行配置校验。
`validate` 接受函数路径。字段和变体固定使用 snake_case；
`rename` 指定单个名称，包括 MiB 等标准符号。

子配置节使用 `#[config(parent = ParentConfig, key = "child")]`。父类型必须是未启用
`deny_unknown_fields` 的 `Config` 具名字段结构体；扩展许可与父字段重名检查均在编译期
完成，支持跨 crate 引用。Key 必须是单个路径段，已有裸点分 key 应改为显式父类型声明。
`ConfigValue` 继续用于配置节内部拥有的字段。

`Config` 同时实现公开的 `ConfigSection` trait，提供完整的 `KEY`。
`LoadedConfig::get<T>()` 要求这个 trait，普通类型和 `ConfigValue` 类型会编译失败。
泛型读取辅助函数可使用 `T: ah_config::ConfigSection`。

使用方式见 [API 参考文档](https://docs.rs/ah-config/latest/ah_config/)、
[使用指南](https://github.com/AzVs32/asset-hub/blob/main/libs/ah-config/README.zh.md)
及[独立案例](https://github.com/AzVs32/asset-hub/tree/main/libs/ah-config/examples)。

采用 Apache-2.0 许可证。
