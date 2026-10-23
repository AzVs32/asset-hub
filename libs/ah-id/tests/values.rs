use ah_id::InvalidIdError;
use std::error::Error;

#[test]
fn invalid_id_error_has_a_stable_public_contract() {
    fn standard_error<T: Error + Send + Sync + 'static>() {}
    standard_error::<InvalidIdError>();
    assert_eq!(InvalidIdError.to_string(), "invalid identifier");
    assert!(InvalidIdError.source().is_none());
}

#[cfg(any(feature = "uuid_v7", feature = "ulid", feature = "nanoid"))]
mod builtins {
    use ah_id::{Generate, InvalidIdError};
    use std::{fmt::Debug, str::FromStr};

    // All three text entry points must agree; valid values must round-trip through display.
    fn parse_consistently<T>(text: &str) -> Result<T, InvalidIdError>
    where
        T: Debug + Eq + ToString + FromStr<Err = InvalidIdError>,
        for<'a> T: TryFrom<&'a str, Error = InvalidIdError>,
        T: TryFrom<String, Error = InvalidIdError>,
    {
        let parsed = text.parse::<T>();
        assert_eq!(T::try_from(text), parsed);
        assert_eq!(T::try_from(text.to_owned()), parsed);
        if let Ok(value) = &parsed {
            assert_eq!(value.to_string().parse::<T>(), parsed);
        }
        parsed
    }

    #[cfg(feature = "uuid_v7")]
    #[test]
    fn uuid_v7_validates_raw_values_and_text() -> Result<(), InvalidIdError> {
        let id = ah_id::UuidV7::generate();
        assert_eq!(id.as_ref().get_version_num(), 7);
        assert_eq!(id.as_ref().get_variant(), uuid::Variant::RFC4122);
        assert_eq!(ah_id::UuidV7::try_from(*id.as_ref())?, id);
        assert_eq!(parse_consistently::<ah_id::UuidV7>(&id.to_string())?, id);
        assert_eq!(
            parse_consistently::<ah_id::UuidV7>(&id.to_string().to_uppercase())?,
            id
        );

        for text in [
            "",
            "invalid-id",
            "00000000-0000-0000-0000-000000000000",
            "ffffffff-ffff-ffff-ffff-ffffffffffff",
            "67e55044-10b1-426f-9247-bb680e5fe0c8",
            "01900000-0000-7000-0247-bb680e5fe0c8",
        ] {
            assert_eq!(
                parse_consistently::<ah_id::UuidV7>(text),
                Err(InvalidIdError)
            );
            if let Ok(raw) = text.parse::<uuid::Uuid>() {
                assert_eq!(ah_id::UuidV7::try_from(raw), Err(InvalidIdError));
            }
        }
        Ok(())
    }

    #[cfg(feature = "ulid")]
    #[test]
    fn ulid_preserves_canonical_text_and_rejects_overflow() -> Result<(), InvalidIdError> {
        let id = ah_id::Ulid::generate();
        assert_eq!(id.to_string().len(), 26);
        assert_eq!(parse_consistently::<ah_id::Ulid>(&id.to_string())?, id);
        assert_eq!(
            parse_consistently::<ah_id::Ulid>("7zzzzzzzzzzzzzzzzzzzzzzzzz")?.to_string(),
            "7ZZZZZZZZZZZZZZZZZZZZZZZZZ"
        );
        for text in [
            "",
            "0",
            "8ZZZZZZZZZZZZZZZZZZZZZZZZZ",
            "ZZZZZZZZZZZZZZZZZZZZZZZZZZ",
            "0000000000000000000000000!",
        ] {
            assert_eq!(parse_consistently::<ah_id::Ulid>(text), Err(InvalidIdError));
        }
        Ok(())
    }

    #[cfg(feature = "nanoid")]
    #[test]
    fn nanoid_checks_length_and_alphabet() -> Result<(), InvalidIdError> {
        let id = ah_id::NanoId::generate();
        let text = id.to_string();
        assert_eq!(text.len(), 21);
        assert!(
            text.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        );
        assert_eq!(parse_consistently::<ah_id::NanoId>(&text)?, id);
        for text in [
            "",
            "short",
            "aaaaaaaaaaaaaaaaaaaaaa",
            "aaaaaaaaaaaaaaaaaaaa!",
            "aaaaaaaaaaaaaaaaaaaé",
        ] {
            assert_eq!(
                parse_consistently::<ah_id::NanoId>(text),
                Err(InvalidIdError)
            );
        }
        Ok(())
    }

    // Share generated values, boundary cases and Unicode inputs across all three ID types.
    fn check_generated_inputs<T>()
    where
        T: Generate + Debug + Eq + ToString + FromStr<Err = InvalidIdError>,
        for<'a> T: TryFrom<&'a str, Error = InvalidIdError>,
        T: TryFrom<String, Error = InvalidIdError>,
    {
        for _ in 0..128 {
            let value = T::generate();
            let text = value.to_string();
            assert_eq!(parse_consistently::<T>(&text), Ok(value));
            for (position, _) in text.char_indices() {
                for replacement in ["!", "\0", "é", "🦀"] {
                    let mut mutated = text.clone();
                    mutated.replace_range(position..position + 1, replacement);
                    let _ = parse_consistently::<T>(&mutated);
                }
            }
        }
        for length in [0, 1, 20, 21, 22, 25, 26, 27, 32, 36, 37, 4096] {
            for ch in ['0', '7', '8', 'Z', '-', '_', '\0', 'é', '🦀'] {
                let _ = parse_consistently::<T>(&ch.to_string().repeat(length));
            }
        }
        for seed in 0..512u32 {
            let text: String = (0..seed % 65)
                .filter_map(|position| char::from_u32((seed * 8191 + position * 65537) % 0x110000))
                .collect();
            let _ = parse_consistently::<T>(&text);
        }
    }

    #[cfg(feature = "uuid_v7")]
    #[test]
    fn uuid_text_entry_points_agree_on_generated_inputs() {
        check_generated_inputs::<ah_id::UuidV7>();
    }

    #[cfg(feature = "ulid")]
    #[test]
    fn ulid_text_entry_points_agree_on_generated_inputs() {
        check_generated_inputs::<ah_id::Ulid>();
    }

    #[cfg(feature = "nanoid")]
    #[test]
    fn nanoid_text_entry_points_agree_on_generated_inputs() {
        check_generated_inputs::<ah_id::NanoId>();
    }
}
