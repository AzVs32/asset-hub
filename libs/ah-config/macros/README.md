# ah-config-macros

Configuration declaration and value derives for `ah-config`. Applications depend
on `ah-config` and use its `Config` and `ConfigValue` re-exports.

`Config` registers a section; `ConfigValue` describes a nested value without
registration. Both generate Serde `Serialize` and `Deserialize`; do not derive
those traits again. Use `#[config(...)]` for
field defaults, enum-level default selection, naming, enum selectors and validation.
`default` accepts a value or call expression, such as `default = factory()`. On enums,
`default = Self::Variant` constructs the selected variant from its field defaults.
For both derives, any type-level default generates `Default`; do not also
implement or derive it. Without a type-level default, a manual or derived
`Default` is allowed but does not implicitly default a section or an unmarked
parent field. Calling `Default::default()` does not run configuration validation.
`validate` accepts a function path. Field and variant names always use snake_case;
`rename` sets one explicit name, including standard symbols such as MiB.

See the [API reference](https://docs.rs/ah-config/latest/ah_config/),
[usage guide](https://github.com/AzVs32/asset-hub/blob/main/libs/ah-config/README.md)
and [examples](https://github.com/AzVs32/asset-hub/tree/main/libs/ah-config/examples).

Licensed under Apache-2.0.
