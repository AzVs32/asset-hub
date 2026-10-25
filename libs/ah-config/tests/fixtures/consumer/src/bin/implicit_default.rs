#[derive(settings::Config)]
#[config(key = "implicit")]
struct Settings {
    #[config(default = 1)]
    value: u16,
}

fn main() {
    let _: Settings = Default::default();
}
