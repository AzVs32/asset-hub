#![cfg(feature = "derive")]

use ah_id::{Generate, InvalidIdError};
use std::{fmt, fmt::Debug, str::FromStr};

#[derive(Debug, PartialEq)]
pub struct TestValue(u32);

impl Generate for TestValue {
    fn generate() -> Self {
        Self(42)
    }
}

impl fmt::Display for TestValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for TestValue {
    type Err = InvalidIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self).map_err(|_| InvalidIdError)
    }
}

// No Clone, Copy, Ord, Hash or Default is required by the macro.
#[derive(ah_id::Id, Debug, PartialEq)]
#[id(generate, display, parse)]
pub struct TestId(TestValue);

fn check_text_interfaces<T>(text: &str)
where
    T: Debug + PartialEq + FromStr<Err = InvalidIdError>,
    for<'a> T: TryFrom<&'a str, Error = InvalidIdError>,
    T: TryFrom<String, Error = InvalidIdError>,
{
    let expected = text.parse::<T>();
    assert_eq!(T::try_from(text), expected);
    assert_eq!(T::try_from(text.to_owned()), expected);
}

#[test]
fn custom_value_works_without_builtin_features() -> Result<(), InvalidIdError> {
    assert_eq!(TestId::new(), "42".parse()?);
    assert_eq!(TestId::generate(), TestId::new());
    check_text_interfaces::<TestId>("42");
    check_text_interfaces::<TestId>("invalid");
    Ok(())
}

#[derive(Debug, PartialEq)]
struct Number(u32);

impl FromStr for Number {
    type Err = std::num::ParseIntError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

#[derive(ah_id::Id, Debug, PartialEq)]
#[id(parse)]
struct ParsedNumber(Number);

#[test]
fn parse_is_independent_of_display_and_preserves_the_value_error()
-> Result<(), std::num::ParseIntError> {
    assert_eq!("12".parse::<ParsedNumber>()?, ParsedNumber(Number(12)));
    let expected = "invalid".parse::<u32>().err();
    assert_eq!("invalid".parse::<ParsedNumber>().err(), expected);
    assert_eq!(ParsedNumber::try_from("invalid").err(), expected);
    assert_eq!(ParsedNumber::try_from("invalid".to_owned()).err(), expected);
    Ok(())
}

#[derive(Debug, PartialEq)]
struct Opaque;

impl Generate for Opaque {
    fn generate() -> Self {
        Self
    }
}

#[derive(ah_id::Id, Debug, PartialEq)]
#[id(generate)]
struct GeneratedOpaque(Opaque);

#[test]
fn generate_is_independent_of_display_and_parsing() {
    assert_eq!(GeneratedOpaque::new(), GeneratedOpaque(Opaque));
    assert_eq!(GeneratedOpaque::generate(), GeneratedOpaque(Opaque));
}

// Run valid usage as ordinary Rust tests; modules only isolate test type names.
mod as_ref {
    struct Value;

    #[derive(ah_id::Id)]
    #[id(as_ref)]
    struct Explicit<T>(T);

    #[test]
    fn explicit_shared_access_needs_no_other_traits() {
        let id = Explicit(Value);
        let _: &Value = id.as_ref();
    }
}

mod deref {
    use super::TestValue;

    #[derive(ah_id::Id)]
    #[id(as_ref, deref)]
    struct Wrapper<T>(T);

    #[derive(ah_id::Id)]
    #[id(generate, display, parse, as_ref, deref)]
    struct Generated(TestValue);

    // Named constructors belong to the domain type when there is no strategy.
    impl<T> Wrapper<T> {
        fn new(value: T) -> Self {
            Self(value)
        }
    }

    #[test]
    fn shared_access_and_method_calls_work_together() {
        let id = Wrapper::new(vec![12, 34]);
        assert_eq!(id.len(), 2);
        let values: &Vec<i32> = id.as_ref();
        assert_eq!(values.as_slice(), &[12, 34]);

        let generated = Generated::new();
        assert_eq!(generated.as_ref().to_string(), "42");
        assert_eq!(generated.to_string(), "42");
    }
}

mod generic {
    use std::{fmt, str::FromStr};

    #[derive(Debug, PartialEq)]
    struct Value<T>(T);

    impl<T: From<u8>> ah_id::Generate for Value<T> {
        fn generate() -> Self {
            Self(T::from(42))
        }
    }

    impl<T: fmt::Display> fmt::Display for Value<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.fmt(f)
        }
    }

    impl<T: FromStr> FromStr for Value<T> {
        type Err = T::Err;

        fn from_str(value: &str) -> Result<Self, Self::Err> {
            value.parse().map(Self)
        }
    }

    #[derive(ah_id::Id, Debug, PartialEq)]
    #[id(generate, display, parse)]
    struct GenericId<T>(Value<T>)
    where
        T: fmt::Display + FromStr + From<u8>;

    #[test]
    fn generic_parameters_and_where_clauses_are_preserved() -> Result<(), std::num::ParseIntError> {
        assert_eq!(GenericId::<u32>::new(), "42".parse()?);
        assert_eq!(GenericId::<u16>::new(), "42".parse()?);
        Ok(())
    }
}

mod hygiene {
    use super::TestValue;

    // Names in the consuming module must not capture names in generated code.
    struct String;
    struct Result;
    struct Option;

    #[derive(ah_id::Id, Debug, PartialEq)]
    #[id(generate, display, parse)]
    struct Id(TestValue);

    #[test]
    fn local_names_do_not_capture_generated_paths() -> std::result::Result<(), ah_id::InvalidIdError>
    {
        let _ = (String, Result, Option);
        assert_eq!(Id::new(), Id::try_from(std::string::String::from("42"))?);
        Ok(())
    }
}

mod non_copy {
    use std::fmt;

    struct Value(std::string::String);

    impl fmt::Display for Value {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.fmt(f)
        }
    }

    impl ah_id::Generate for Value {
        fn generate() -> Self {
            Value("example".to_owned())
        }
    }

    impl std::str::FromStr for Value {
        type Err = ah_id::InvalidIdError;

        fn from_str(value: &str) -> Result<Self, Self::Err> {
            Ok(Value(value.to_owned()))
        }
    }

    #[derive(ah_id::Id)]
    #[id(generate, display, parse)]
    struct Id(Value);

    #[test]
    fn owned_values_need_no_clone_or_copy() -> Result<(), ah_id::InvalidIdError> {
        assert_eq!(Id::new().to_string(), "example");
        assert_eq!("restored".parse::<Id>()?.to_string(), "restored");
        Ok(())
    }
}

mod custom_capabilities {
    use std::{fmt, str::FromStr};

    struct Value;

    #[derive(ah_id::Id)]
    #[id(deref)]
    struct Id(Value);

    // Unselected abilities remain available for domain-owned implementations.
    impl Id {
        fn new(value: Value) -> Self {
            Self(value)
        }
    }

    impl fmt::Display for Id {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("stored")
        }
    }

    impl FromStr for Id {
        type Err = ();

        fn from_str(value: &str) -> Result<Self, Self::Err> {
            if value != "stored" {
                return Err(());
            }
            Ok(Self::new(Value))
        }
    }

    #[test]
    fn unselected_capabilities_allow_manual_implementations() -> Result<(), ()> {
        let id: Id = "stored".parse()?;
        assert_eq!(id.to_string(), "stored");
        assert!("other".parse::<Id>().is_err());
        Ok(())
    }
}
