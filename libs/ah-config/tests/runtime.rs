use ah_config::{ConfigErrorKind as Kind, Registry};

#[test]
fn a_program_without_declarations_reports_unknown_spaces() {
    assert!(Registry::spaces().is_empty());
    assert!(Registry::declarations().is_empty());
    let error = Registry::for_space("default").err();
    assert!(
        error.is_some_and(|error| error.available_spaces().is_empty()
            && error.to_string().contains("available spaces: []"))
    );
    assert_eq!(
        Registry::for_space("default")
            .err()
            .map(|error| error.kind()),
        Some(Kind::UnknownSpace)
    );
    assert_eq!(
        Registry::for_space("").err().map(|error| error.kind()),
        Some(Kind::InvalidSpace)
    );
}
