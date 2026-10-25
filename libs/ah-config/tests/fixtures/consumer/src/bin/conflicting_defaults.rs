#[derive(settings::Config)]
#[config(key = "conflicting")]
struct Settings {
    #[config(default = 1, default = default_value())]
    value: u16,
}
fn default_value() -> u16 {
    2
}
fn main() {}
