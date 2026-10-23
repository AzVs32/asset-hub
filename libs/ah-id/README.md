# ah-id

[简体中文](README.zh.md)

Strongly typed identifiers with validated UUID v7, ULID and NanoID values, plus a derive macro for custom value types.

## Usage

Add the dependency to your `Cargo.toml`, enabling the value type you need:

```toml
[dependencies]
ah-id = { version = "0.1.0", features = ["nanoid"] }
```

| Feature | Provides |
| --- | --- |
| `derive` (default) | `#[derive(ah_id::Id)]` |
| `uuid_v7` | `UuidV7` |
| `ulid` | `Ulid` |
| `nanoid` | `NanoId` |

Set `default-features = false` to use built-in values without the macro. `Generate` and `InvalidIdError` are always available.

The derive macro requires one private tuple field and at least one option:

| `#[id(...)]` option | Generated API |
| --- | --- |
| `generate` | `new()` and `ah_id::Generate`, delegated to the value |
| `display` | `Display`, delegated to the value |
| `parse` | `FromStr`, `TryFrom<&str>` and `TryFrom<String>`, preserving the value's parser error |
| `as_ref` | `AsRef<Value>` |
| `deref` | Shared `Deref<Target = Value>` |

Options are independent. Choose standard derives such as `Clone`, `Copy`, `Eq` and `Hash` yourself. The macro adds no mutable access or `Default`.

Generated parsing and generation directly wrap a valid value; they do not call a handwritten `TryFrom<Value>`. Enforce additional ID constraints in the wrapped type or implement those input paths manually. Text formats belong to the value's `Display` and `FromStr` implementations.

### Examples

Each file contains an explanation and instructions for running the example.

| Example | Demonstrates |
| --- | --- |
| [NanoID](examples/nanoid.rs) | Generate and restore values and typed IDs |
| [UUID v7](examples/uuid_v7.rs) | UUID v7 values and a typed mount ID |
| [ULID](examples/ulid.rs) | ULID values and a typed event ID |
| [Vector](examples/vector.rs) | Wrap an existing type, validate inputs and access its fields and methods |
| [Order line](examples/order_line.rs) | A composite key using a NanoID order ID and a line number |

## Maintenance

The commands below run from the ah-id source root, the directory containing this README and `Cargo.toml`. 

Maintenance tools require a source checkout; the published package does not contain the complete tooling.

- `src/` contains the public API; `macros/src/` implements the derive macro.
- `examples/` contains usage and behavior tests; `tests/derive.rs` and `tests/values.rs` cover macro behavior and value validation.
- `tests/fixtures/` contains standalone consumer projects for integration checks.

### API documentation

```sh
cargo doc --all-features --no-deps
```

The generated documentation is at `target/doc/ah_id/index.html`; open it in a browser. If `CARGO_TARGET_DIR` is set, the page is under `doc/ah_id/index.html` in that directory.

### Checks

Install Rust with `rustfmt` and `clippy`, plus Python 3.12+. The complete check script requires the shared `.github/checks/` tools in an ancestor directory of the source checkout:

```sh
python3 ci/check.py
```

This checks formatting, feature combinations, examples, dependency renaming, Clippy, API documentation and packaged crates. It does not publish. Logs are in `target/ci-logs/`; a nonzero exit code indicates failure or interruption.

Use `--docker` for all four local Linux environments or, for example, `--docker arch` for one. Docker checks need daemon access and network access to pull images and install dependencies. GitHub CI checks Windows and macOS. The script defines package checks; `.github/checks/` provides shared execution and logging.

For a source copy without the shared tools, run Cargo directly:

```sh
cargo test --workspace --all-features --locked
```

## License

Copyright 2026 AzVs. Licensed under the [Apache License 2.0](LICENSE).
