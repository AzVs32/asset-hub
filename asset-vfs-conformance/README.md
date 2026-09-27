# asset-vfs-conformance

Reusable assertion functions for `asset-vfs` implementations. Add this crate
under `[dev-dependencies]`; `asset-vfs` does not depend on it.

All current checks live under `asset_vfs_conformance::driver`:

| Module | Subject | Fixture data |
| --- | --- | --- |
| `binding` | `Driver` | `binding::TREE` and a native relative-path mapping |
| `bound_driver` | `BoundDriver` | Two simultaneous bindings from the same driver, populated from `TREE` and `OTHER_TREE` |
| `read_driver` | `ReadDriver` | `read_driver::TREE` |
| `data` | Native test setup | Shared `TreeSpec`, `TreeEntry`, and `TreeBuilder` |

The trees in `src/driver/data.rs` are the source of truth. Directory and file
paths and file contents are described once; setup and expected listings use the
same descriptions. Byte lengths are derived from contents, and listings are
sorted automatically. Adding a directory or changing file contents normally
requires editing only the shared data, not each driver's fixtures or assertions.

Implement `data::TreeBuilder` once per backend:

- `create_directory`: map a relative path beneath the chosen native root and
  create that directory. Creating an existing directory succeeds.
- `write_file`: map the relative path and store the supplied bytes.

Then each fixture prepares its data with a single call:

```rust,ignore
read_driver::TREE.populate(&mut builder)?;
```

`populate` creates the implicit root, creates directories in parent-first order,
and then writes files. It returns the first native error unchanged. The builder
is test setup and does not require production `WriteDriver` support. Read-only
remote fixtures may instead use pre-provisioned data matching the shared tree.

The binding fixture implements `driver_path(&VirtualRelativePath)` to translate
relative paths to native `DriverPath` values. The directory root, file root, and
missing root are derived by default, so their file names need not be duplicated
in the implementation's test code.

Each interface module exports its own `Fixture` trait and `check_*` functions.
Fixtures own their resources, must not interfere with other fixture instances,
and must keep their contents stable during a check. Use a fresh fixture per test:

```rust,ignore
use asset_vfs_conformance::driver::read_driver::{self, Fixture};

#[test]
fn listing_order() {
    let fixture = MyReadFixture::new();
    read_driver::check_listing_order(&fixture);
}
```

A single environment may implement multiple fixture traits, or each suite may
use its own smaller environment. Read fixtures need not expose a driver factory
or a bound backend. Fixtures expose trait objects, allowing integration tests
to use private backend implementations through their public interfaces.

Invalid root syntax is driver-specific. Opt into `binding::check_bind_invalid_root`
with an explicitly invalid `DriverPath`. Writer exposure is also explicit:
`bound_driver::check_writer_presence(&fixture, true)` or `false`. No other check
assumes that writing is available. `WriteDriver` has no operations yet, so there
is no write-operation suite.

Backend-specific cases (symbolic links, unrepresentable native names, permission
errors, concurrent storage changes) belong in the implementation's own tests.
No macros or automatic test generation are provided at this stage. See
`infra/tests/local_driver.rs` and `infra/tests/memory_driver.rs` in the workspace
for native builders, fixtures, and independently selected tests.
