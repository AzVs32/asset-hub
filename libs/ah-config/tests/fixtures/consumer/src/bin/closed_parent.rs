#[derive(settings::Config)]
#[config(parent = config_extension::ClosedVfsConfig, key = "wasm")]
struct WasmConfig {
    memory_limit_mb: u64,
}

fn main() {}
