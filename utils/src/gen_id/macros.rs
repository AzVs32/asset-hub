/// Declares a strongly typed identifier with generation, display and parsing.
///
/// This macro is always available and requires an explicit strategy. Enable the
/// `uuid_v7`, `ulid` or `nanoid` feature for the corresponding implementation.
/// Each identifier implements `Copy`, comparison and hashing, and parses with
/// [`crate::ParseIdError`]. Nano IDs use 21 URL-safe characters.
///
/// Parsing validates the selected strategy's representation. Invalid input
/// returns `ParseIdError` without retaining the input or a backend error. The
/// calling operation is responsible for mapping it to a business error.
///
/// ```
/// # #[cfg(feature = "uuid_v7")]
/// # {
/// utils::gen_id!(utils::UUID_v7, UserId);
///
/// let id = UserId::new();
/// assert_eq!(id.to_string().parse::<UserId>().unwrap(), id);
/// # }
/// # #[cfg(feature = "ulid")]
/// # {
/// utils::gen_id!(utils::ULID, EventId);
/// # }
/// # #[cfg(feature = "nanoid")]
/// # {
/// utils::gen_id!(utils::NanoID, TokenId);
/// # }
/// ```
#[macro_export]
macro_rules! gen_id {
    ($strategy:path, $name:ident $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name($strategy);

        impl $name {
            /// Creates a new identifier using the selected strategy.
            pub fn new() -> Self {
                Self(<$strategy as $crate::__private::IdStrategy>::generate())
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                ::std::fmt::Display::fmt(&self.0, f)
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::ParseIdError;

            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                <$strategy as $crate::__private::IdStrategy>::parse(s).map(Self)
            }
        }
    };
}
