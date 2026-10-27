#[derive(settings::ConfigValue)]
struct Nested {
    port: u16,
}

fn read(config: &settings::LoadedConfig) {
    let _ = config.get::<Nested>();
}

fn main() {}
