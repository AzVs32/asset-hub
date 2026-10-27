//! Extend a namespace without adding child fields to its original Rust type.
//!
//! Each Config owns a section and is retrieved independently. ConfigValue is for
//! fields owned by one section, such as Limits below; it is not registered.
//! A child names its parent type, which may come from another crate. Parents must
//! be named Config structs without deny_unknown_fields. Closed parents, dotted
//! keys and child keys matching a parent field are rejected during compilation.
//!
//! Run from the ah-config source root: `cargo run --example extensions`.

use ah_config::Registry;

mod vfs {
    #[derive(ah_config::Config)]
    #[config(key = "vfs", default)]
    pub struct VfsConfig {
        #[config(default = true)]
        pub enabled: bool,
    }
}

mod rpc {
    #[derive(ah_config::Config)]
    #[config(parent = super::vfs::VfsConfig, key = "rpc", default)]
    pub struct RpcConfig {
        #[config(default = 5000)]
        pub timeout_ms: u64,
    }
}

mod wasm {
    #[derive(ah_config::Config)]
    #[config(parent = super::vfs::VfsConfig, key = "wasm", default, validate = validate_wasm)]
    pub struct WasmConfig {
        #[config(default)]
        pub limits: Limits,
    }

    #[derive(ah_config::ConfigValue)]
    #[config(default, deny_unknown_fields)]
    pub struct Limits {
        #[config(default = 128)]
        pub memory_limit_mb: u64,
    }

    fn validate_wasm(config: &WasmConfig) -> Result<(), &'static str> {
        if config.limits.memory_limit_mb == 0 {
            Err("WASM memory limit must be nonzero")
        } else {
            Ok(())
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Registry::for_space("default")?.load_required("examples/extensions.toml")?;
    assert!(config.get::<vfs::VfsConfig>()?.enabled);
    assert_eq!(config.get::<rpc::RpcConfig>()?.timeout_ms, 3000);
    assert_eq!(
        config.get::<wasm::WasmConfig>()?.limits.memory_limit_mb,
        256
    );
    println!("Loaded vfs, vfs.rpc and vfs.wasm independently.");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn loads_independent_extensions() -> Result<(), Box<dyn std::error::Error>> {
        super::main()
    }

    #[test]
    fn validates_child_configuration() -> Result<(), Box<dyn std::error::Error>> {
        let error = super::Registry::for_space("default")?
            .load_required("examples/extensions_invalid.toml")
            .err()
            .ok_or("expected an invalid memory limit")?;
        assert_eq!(error.kind(), ah_config::ConfigErrorKind::Validation);
        assert_eq!(error.section(), Some("vfs.wasm"));
        Ok(())
    }
}
