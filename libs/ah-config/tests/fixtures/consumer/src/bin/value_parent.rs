#[derive(settings::ConfigValue)]
struct VfsValue {}

#[derive(settings::Config)]
#[config(parent = VfsValue, key = "wasm")]
struct WasmConfig {}

fn main() {}
