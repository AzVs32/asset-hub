#[derive(settings::ConfigValue)]
#[config(default = defaults())]
struct Settings {
    value: u16,
}

fn defaults() -> u16 {
    42
}

fn main() {}
