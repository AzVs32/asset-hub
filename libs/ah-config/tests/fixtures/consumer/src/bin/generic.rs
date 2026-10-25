#[derive(settings::Config)]
#[config(key = "generic")]
struct Settings<T> {
    value: T,
}
fn main() {}
