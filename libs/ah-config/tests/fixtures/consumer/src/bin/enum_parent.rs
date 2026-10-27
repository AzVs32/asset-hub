#[derive(settings::Config)]
#[config(key = "vfs", tag = "kind")]
enum VfsConfig {
    Memory,
}

#[derive(settings::Config)]
#[config(parent = VfsConfig, key = "wasm")]
struct WasmConfig {}

fn main() {}
