#[derive(settings::Config, Default)]
#[config(key = "conflict", default = Self { value: 1 })]
struct Settings {
    value: u16,
}

fn main() {}
