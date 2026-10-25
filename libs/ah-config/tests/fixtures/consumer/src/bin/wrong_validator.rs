#[derive(settings::Config)]
#[config(key = "wrong_validator", validate = validate)]
struct Settings {
    value: u16,
}
fn validate(_: &Settings) -> bool {
    true
}
fn main() {}
