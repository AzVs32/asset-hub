#[derive(settings::Config)]
#[config(key = "vfs")]
struct VfsConfig {
    #[config(rename = "wasm")]
    embedded: WasmValue,
}

#[derive(settings::ConfigValue)]
struct WasmValue {
    memory_limit_mb: u64,
}

#[derive(settings::Config)]
#[config(parent = VfsConfig, key = "wasm")]
struct WasmConfig {
    memory_limit_mb: u64,
}

fn main() {}
