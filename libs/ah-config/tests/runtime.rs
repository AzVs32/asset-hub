use ah_config::{ConfigErrorKind as Kind, Registry};

#[test]
fn a_program_without_declarations_reports_unknown_spaces() {
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
