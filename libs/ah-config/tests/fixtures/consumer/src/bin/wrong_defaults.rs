#[derive(settings::Config)]
#[config(key = "wrong_defaults", default = defaults())]
struct Settings {
    value: u16,
}
fn defaults() -> u16 {
    42
}
fn main() {}
