#[derive(settings::ConfigValue)]
#[config(tag = "backend", default = Self::Memory)]
enum Storage {
    Memory { capacity: u64 },
}

fn main() {}
