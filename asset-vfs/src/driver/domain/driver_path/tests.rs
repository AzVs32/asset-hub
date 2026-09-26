use super::DriverPath;

#[test]
fn driver_path_preserves_driver_specific_syntax() {
    let raw = r"bucket/prefix\\..//object";
    let path = DriverPath::new(raw);

    assert_eq!(path.as_str(), raw);
}

#[test]
fn driver_path_allows_an_empty_driver_root() {
    assert_eq!(DriverPath::new("").as_str(), "");
}
