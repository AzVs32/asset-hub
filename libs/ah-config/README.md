# ah-config

[简体中文](README.zh.md)

Typed TOML configuration with isolated spaces, validated snapshots and a derive macro for application-owned configuration types.

## Usage

Add the dependencies to your `Cargo.toml`:

```toml
[dependencies]
ah-config = "0.1.0"
```

| Feature | Provides |
| --- | --- |
| `derive` (default) | `#[derive(ah_config::Config)]` and `#[derive(ah_config::ConfigValue)]` |

Set `default-features = false` to use the loading API without the macro. `Registry`, `LoadedConfig`, `ConfigError` and `ConfigErrorKind` are always available.

`Config` declares a section and requires a key. `ConfigValue` declares a nested struct or enum without registering a section. Both generate Serde `Serialize` and `Deserialize`; do not derive those traits again. Standard derives such as `Clone` and `Debug` remain your choice.

| `#[config(...)]` option | Declared behavior |
| --- | --- |
| `key = "http"` | Required TOML section path; supports dotted paths such as `service.http` |
| `spaces = ["default", "plugins"]` | Space membership; defaults to only `default` when omitted |
| `default = expression` | Type: generate `Default`; field: supply a missing value. Functions use `default = factory()` |
| `default` | Field: use `Default`; struct: generate `Default` from field declarations |
| `default = Self::Variant` | On an enum: select a default variant and generate `Default` from its field declarations |
| `validate = check` | Section validator accepting `&T` and returning `Result<(), E>` |
| `tag = "backend"` | Selector field for an enum with payloads; unsupported on structs |
| `rename = "name"` | External name of one field or variant |
| `deny_unknown_fields` | Opt into strict field checking |

Fields and enum variants always use `snake_case`: `LocalFile` becomes `local_file`. Use a single-item `rename` for standard symbols such as `MiB` or other special names. Explicit section keys, tag names and string values are used as supplied.

Both `Config` and `ConfigValue` generate `Default` for every type-level `default` declaration. Bare `default` on a struct constructs it from field defaults; `default = expression` returns the expression, including a function call. An enum-level `default = Self::Variant` constructs the selected variant from its field defaults. Automatic field construction requires explicit defaults or `Option<T>`. Do not also implement or derive `Default` when a type-level default is declared; put custom construction logic in a factory instead. Factories must not call the generated `Default` recursively.

`Config` uses that generated implementation as its section's loading defaults. Without a type-level declaration, a manual or derived `Default` is allowed but does not automatically become loading defaults. A nested type implementing `Default` does not by itself default an absent parent field; opt in with `#[config(default)]` or supply an explicit default expression. Direct `Default::default()` calls construct values without running configuration validation.

Missing fields use available inherited or field defaults; `Option<T>` becomes `None`, and fields without an available default remain required. Merely implementing `Default` for a field type does not make that field optional. A partially supplied nested struct uses its field defaults rather than implicitly invoking its type's `Default`; inherited parent defaults still merge recursively.

A struct section without a type default can be built from field defaults and optional fields. Missing required fields still fail. Struct type defaults merge recursively with file fields; arrays and explicitly supplied enum values are replaced at any nesting depth. An omitted enum retains its inherited default. A supplied enum must include its selector and required fields; its own field defaults still apply, even when selecting the same variant. An explicit invalid value never falls back to a default. Only one `default` declaration is allowed at each location. Function calls use normal Rust expression syntax with parentheses; bare function names are not invoked implicitly. Validator errors must convert to `Box<dyn Error + Send + Sync>`.

Select a space with `Registry::for_space(name)`, load a TOML file with `load_required(path)` or `load_optional(path)`, then read a shared `Arc<T>` with `get::<T>()`. Explicit spaces replace the default membership; space names do not prefix TOML paths. All selected configurations must load and pass validation before a snapshot is returned.

Pass a relative file path such as `"config/config.toml"` directly to either loading method. Cargo runs and tests automatically use the package directory, including Release runs. When the program is launched directly without Cargo's runtime environment, relative paths start at the executable's directory. Both are independent of the working directory. Absolute file paths are used unchanged. For a custom location, call `with_base_dir(absolute_directory)` before loading; it overrides the automatic base. Relative or empty bases fail when loading a relative file path. Paths stored inside configuration values are not rewritten.

An optional file may be missing; other loading failures are still errors. Each load produces an independent snapshot. File changes do not update existing values; load again to obtain new values. Default merging, validation and error handling are covered in the examples below.

Use `ConfigError::kind()` to classify a failure and `path()`, `section()` and `space()` to locate it. Location methods return `Option`: preparation failures have no file path, and file-level failures may have no registered section. `Error::source()` retains the underlying diagnostics; `Display` and `Debug` omit configuration values and underlying causes. A non-table ancestor of a section is a `Deserialize` error, rather than `MissingSection`.

### Examples

Read the examples in the order below. Each addresses one configuration task and includes its run command. Read each Rust file alongside the TOML files with the same filename prefix.

The first four examples incorporate validation into their configuration declarations and include invalid fixtures exercised by their tests: conversion errors reject invalid types, while validators check the assembled values, including defaults.

| Example | Demonstrates |
| --- | --- |
| [Basic](examples/basic.rs) | The declare → select → load → get flow, required and optional fields, nested values with `Config` and `ConfigValue`, and section validation |
| [Defaults](examples/defaults.rs) | Value expressions, function calls and `Default`, nested values and partial overrides, with invalid values rejected rather than replaced |
| [Database](examples/database.rs) | Enum selectors, variant-specific required fields, enum-level defaults and validation of the selected variant |
| [Spaces](examples/spaces.rs) | Private and shared declarations in independently selected spaces, with validation limited to selected declarations |
| [Snapshots](examples/snapshots.rs) | Retain old shared values while explicitly loading a new snapshot |

## Maintenance

The commands below run from the ah-config source root, the directory containing this README and `Cargo.toml`.

- `src/` contains the public API; `macros/src/` implements the derive macro.
- `examples/` contains usage and behavior tests; `tests/loading.rs` and `tests/spaces.rs` cover loading rules and space isolation, `tests/errors.rs` checks error context and causes, `tests/defaults.rs` covers default rules, `tests/nested_defaults.rs` checks nested enum replacement, `tests/naming.rs` checks naming and explicit overrides, `tests/paths.rs` checks file resolution and working-directory independence, and `tests/runtime.rs` checks loading without declarations.
- `tests/fixtures/` contains a standalone consumer project for integration checks.

### API documentation

The [API reference](https://docs.rs/ah-config/latest/ah_config/) describes generated implementations, attribute locations and type restrictions. To build it locally:

```sh
cargo doc --all-features --no-deps
```

The generated documentation is at `target/doc/ah_config/index.html`; open it in a browser. If `CARGO_TARGET_DIR` is set, the page is under `doc/ah_config/index.html` in that directory.

### Checks

Install Python 3.12+ and Rust with `rustfmt` and `clippy`, then run the unified checks from the full repository checkout:

```sh
python3 ci/check.py
```

The entry point checks tests and examples, feature combinations, formatting, Clippy and documentation. It also runs the renamed-dependency consumer and external-crate registration in Debug and Release/LTO, verifies expected compilation failures (including conflicting defaults and duplicate `Default` implementations), and builds the actual `.crate` archives after unpacking them. The unpacked runtime uses the unpacked macro package through a temporary local patch. Nothing is published.

Checks stop on failure with a nonzero exit status. The full log is saved at `target/ci-logs/native.log`; `CARGO_NET_OFFLINE=true` can be set when dependencies are already cached. Execution and logging reuse the repository's `.github/checks` support, as in ah-id; this command does not require changing CI workflows.

An independent source copy can still use Cargo directly, for example `cargo test --workspace --all-features --locked`. Ordinary `cargo test` does not run the independent consumer checks or archive verification.

## License

Copyright 2026 AzVs. Licensed under the [Apache License 2.0](./LICENSE).
