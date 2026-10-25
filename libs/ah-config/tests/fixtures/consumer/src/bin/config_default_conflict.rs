#[derive(settings::Config)]
#[config(key = "conflict", default = Self { value: 1 })]
struct Settings {
    value: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self { value: 2 }
    }
}

fn main() {}
