#[cfg(any(feature = "uuid_v7", feature = "ulid", feature = "nanoid"))]
use std::collections::HashSet;

// The macro must be importable even without a generation implementation enabled.
#[allow(unused_imports)]
use utils::gen_id;

#[cfg(feature = "uuid_v7")]
gen_id!(utils::UUID_v7, UserId);
#[cfg(feature = "ulid")]
gen_id!(utils::ULID, EventId);
#[cfg(feature = "nanoid")]
gen_id!(utils::NanoID, TokenId);

#[cfg(any(feature = "uuid_v7", feature = "ulid", feature = "nanoid"))]
fn check_ids<T>(generate: fn() -> T)
where
    T: Copy + Eq + Ord + std::hash::Hash + std::fmt::Debug + std::fmt::Display,
    T: std::str::FromStr<Err = utils::ParseIdError>,
{
    assert_eq!("invalid-id".parse::<T>(), Err(utils::ParseIdError));
    assert_eq!(
        (0..256).map(|_| generate()).collect::<HashSet<_>>().len(),
        256
    );
}

#[cfg(feature = "uuid_v7")]
#[test]
fn uuid_ids_generate_v7_and_round_trip() {
    check_ids(UserId::new);
    let id = UserId::new();
    let text = id.to_string();
    assert_eq!(text.parse::<UserId>().unwrap(), id);
    let uuid: utils::UUID_v7 = text.parse().unwrap();
    assert_eq!(uuid.get_version_num(), 7);
}

#[cfg(feature = "ulid")]
#[test]
fn ulids_generate_canonical_values_and_round_trip() {
    check_ids(EventId::new);
    let id = EventId::new();
    let text = id.to_string();
    assert_eq!(text.len(), 26);
    assert!(
        text.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    );
    assert_eq!(text.parse::<EventId>().unwrap(), id);
    for valid in ["7ZZZZZZZZZZZZZZZZZZZZZZZZZ", "7zzzzzzzzzzzzzzzzzzzzzzzzz"] {
        assert_eq!(
            valid.parse::<EventId>().unwrap().to_string(),
            "7ZZZZZZZZZZZZZZZZZZZZZZZZZ"
        );
    }
    for invalid in [
        "8ZZZZZZZZZZZZZZZZZZZZZZZZZ",
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZ",
        "0",
        "0000000000000000000000000!",
    ] {
        assert_eq!(invalid.parse::<EventId>(), Err(utils::ParseIdError));
    }
}

#[cfg(feature = "nanoid")]
#[test]
fn nanoids_generate_url_safe_values_and_validate_input() {
    check_ids(TokenId::new);
    let id = TokenId::new();
    let text = id.to_string();
    assert_eq!(text.len(), 21);
    assert!(
        text.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    );
    assert_eq!(text.parse::<TokenId>().unwrap(), id);
    for invalid in [
        "",
        "short",
        "aaaaaaaaaaaaaaaaaaaaaa",
        "aaaaaaaaaaaaaaaaaaaa!",
        "aaaaaaaaaaaaaaaaaaaé",
    ] {
        assert_eq!(invalid.parse::<TokenId>(), Err(utils::ParseIdError));
    }
}
