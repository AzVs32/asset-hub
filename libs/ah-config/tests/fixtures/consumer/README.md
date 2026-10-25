# Independent consumer checks

This unpublished fixture uses `ah-config` as `settings` and links a declaration-only
extension crate. Neither consumer crate directly depends on Serde.

All checks below are automated by `python3 ci/check.py` from the ah-config source
root in the full repository. The entry point also validates the expected compiler
diagnostics, so unrelated Cargo failures cannot count as successful negative tests.

To run individual checks from this fixture directory, run the valid binary in
debug and release (LTO) modes:

```sh
cargo run --bin ah-config-consumer --locked --offline
cargo run --bin ah-config-consumer --release --locked --offline
```

Each command below must fail to compile, checking the public macro contract:

```sh
cargo check --bin conflicting_defaults --locked --offline
cargo check --bin wrong_defaults --locked --offline
cargo check --bin wrong_validator --locked --offline
cargo check --bin generic --locked --offline
cargo check --bin config_default_conflict --locked --offline
cargo check --bin config_derived_default_conflict --locked --offline
cargo check --bin value_default_conflict --locked --offline
cargo check --bin value_derived_default_conflict --locked --offline
cargo check --bin missing_default_field --locked --offline
cargo check --bin missing_variant_default_field --locked --offline
cargo check --bin wrong_value_default --locked --offline
cargo check --bin implicit_default --locked --offline
cargo check --bin implicit_value_default --locked --offline
```

The default checks cover duplicate implementations for both derives, required
fields in automatically constructed defaults, expression return types, and the
absence of an implicit `Default` when only field defaults are declared. The valid
binary also uses the registered section's generated `Default` directly.

Select a binary explicitly when checking this fixture; checking all binaries also
selects the intentionally invalid inputs. No command publishes.
