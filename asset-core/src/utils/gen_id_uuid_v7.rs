/// 生成基于 UUID v7 的强类型 ID。
///
/// `$name`：生成的 ID 类型，例如 `NodeId`。
/// 可选的 `$slot`：生成 crate 内可见的保留槽位，例如 `NodeIdSlot`。
///
/// # 约定
///
/// - 普通 ID 只生成 UUID v7 强类型包装；
/// - 带 `$slot` 的 ID
///     - 把 `Slot0` 映射到 `uuid::Uuid::nil()`
///     - 把 `Slot1` 映射到 `uuid::Uuid::max()`
///
/// # Example
///
/// ```ignore
/// gen_id_uuid_v7!(Node1Id);
/// gen_id_uuid_v7!(Node2Id, Node2IdSlot);
/// ```
#[macro_export]
macro_rules! gen_id_uuid_v7 {
    ($name:ident) => {
        $crate::gen_id_uuid_v7!(@id $name);
    };

    ($name:ident, $slot:ident) => {
        $crate::gen_id_uuid_v7!(@id $name);

        /// 当前 ID 类型预留的特殊槽位。
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub(crate) enum $slot {
            Slot0,
            Slot1,
        }

        impl $name {
            /// 根据保留槽位创建 ID。
            pub(crate) fn from_slot(slot: $slot) -> Self {
                match slot {
                    $slot::Slot0 => Self(uuid::Uuid::nil()),
                    $slot::Slot1 => Self(uuid::Uuid::max()),
                }
            }

            /// 如果当前 ID 对应某个保留槽位，则返回该槽位。
            pub(crate) fn slot(self) -> Option<$slot> {
                if self.0 == uuid::Uuid::nil() {
                    Some($slot::Slot0)
                } else if self.0 == uuid::Uuid::max() {
                    Some($slot::Slot1)
                } else {
                    None
                }
            }
        }
    };

    (@id $name:ident) => {
        /// 基于 UUID v7 的强类型 ID。
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        pub struct $name(uuid::Uuid);

        impl $name {
            /// 创建一个新的 ID。
            pub fn new() -> Self {
                Self(uuid::Uuid::now_v7())
            }

            /// 从原始 UUID 构造强类型 ID。
            pub fn from_uuid(id: uuid::Uuid) -> Self {
                Self(id)
            }

            /// 借用底层 UUID。
            pub fn as_uuid(&self) -> &uuid::Uuid {
                &self.0
            }

            /// 消耗当前 ID 并返回底层 UUID。
            pub fn into_uuid(self) -> uuid::Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl From<uuid::Uuid> for $name {
            fn from(id: uuid::Uuid) -> Self {
                Self::from_uuid(id)
            }
        }

        impl From<$name> for uuid::Uuid {
            fn from(id: $name) -> Self {
                id.into_uuid()
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(uuid::Uuid::parse_str(s)?))
            }
        }
    };
}
