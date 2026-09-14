/// Generate a strongly typed `ID` based on `UUID v7`
/// + `$name`：required, The type of the generated `ID`.
/// + `$slot`：optional, Generate reserved slots visible within the `crate`.
///     + `Slot0` maps to `uuid::Uuid::nil()`
///     + `Slot1` maps to `uuid::Uuid::max()`
#[macro_export]
macro_rules! gen_id_uuid_v7 {
    ($name:ident) => {
        $crate::gen_id_uuid_v7!(@id $name);
    };

    ($name:ident, $slot:ident) => {
        $crate::gen_id_uuid_v7!(@id $name);

        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub(crate) enum $slot {
            Slot0,
            Slot1,
        }

        impl $name {
            /// Create an `ID` based on the `slot` position.
            pub(crate) fn from_slot(slot: $slot) -> Self {
                match slot {
                    $slot::Slot0 => Self($crate::__private::uuid::Uuid::nil()),
                    $slot::Slot1 => Self($crate::__private::uuid::Uuid::max()),
                }
            }

            /// If the current `ID` corresponds to a `slot`, return that `slot`.
            pub(crate) fn to_slot(&self) -> ::std::option::Option<$slot> {
                if self.0 == $crate::__private::uuid::Uuid::nil() {
                    ::std::option::Option::Some($slot::Slot0)
                } else if self.0 == $crate::__private::uuid::Uuid::max() {
                    ::std::option::Option::Some($slot::Slot1)
                } else {
                    ::std::option::Option::None
                }
            }
        }
    };

    (@id $name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name($crate::__private::uuid::Uuid);

        impl $name {
            /// Creates a new identifier.
            pub fn new() -> Self {
                Self($crate::__private::uuid::Uuid::now_v7())
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                ::std::fmt::Display::fmt(&self.0, f)
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::UtilsError;
            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                $crate::__private::uuid::Uuid::parse_str(s)
                    .map(Self)
                    .map_err(|_| $crate::ParseIdError.into())
            }
        }

        impl $crate::__private::serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S)
                -> ::std::result::Result<S::Ok, S::Error>
            where
                S: $crate::__private::serde::Serializer,
            {
                serializer.collect_str(self)
            }
        }

        impl<'de> $crate::__private::serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D)
                -> ::std::result::Result<Self, D::Error>
            where
                D: $crate::__private::serde::Deserializer<'de>,
            {
                let value = <::std::string::String as
                    $crate::__private::serde::Deserialize<'de>
                >::deserialize(deserializer)?;
                <Self as ::std::str::FromStr>::from_str(&value)
                    .map_err(<D::Error as $crate::__private::serde::de::Error>::custom)
            }
        }
    };
}
