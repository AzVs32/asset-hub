#[derive(settings::ConfigValue, Default)]
#[config(default = Self { value: 1 })]
struct Settings {
    value: u16,
}

fn main() {}
