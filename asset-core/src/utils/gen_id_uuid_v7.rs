/// 生成基于 UUID v7 的强类型 ID。
///
/// `$name`：生成的 ID 类型，例如 `DirectoryId`。
/// `$slot`：生成的保留槽位类型，例如 `DirectoryIdSlot`。
///
/// # 约定
///
/// - 正常 ID 通过 UUID v7 生成；
/// - `Slot0` 映射到 UUID nil；
/// - `Slot1` 映射到 UUID max；
/// - 槽位本身不携带任何领域语义，具体用途由上层领域定义。
///
/// # Example
///
/// ```ignore
/// gen_id_uuid_v7!(DirectoryId, DirectoryIdSlot);
///
/// let id = DirectoryId::new();
/// assert!(id.slot().is_none());
///
/// let reserved = DirectoryId::from_slot(DirectoryIdSlot::Slot0);
/// assert_eq!(reserved.slot(), Some(DirectoryIdSlot::Slot0));
/// ```
#[macro_export]
macro_rules! gen_id_uuid_v7 {
    ($name:ident, $slot:ident) => {
        /// 当前 ID 类型预留的特殊槽位。
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $slot {
            Slot0,
            Slot1,
        }

        /// 基于 UUID v7 的强类型 ID。
        ///
        /// 正常实例通过 UUID v7 生成，同时保留少量特殊值供领域层使用。
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        pub struct $name(uuid::Uuid);

        impl $name {
            /// 创建一个新的 UUID v7 ID。
            pub fn new() -> Self {
                Self(uuid::Uuid::now_v7())
            }

            /// 根据保留槽位创建 ID。
            pub fn from_slot(slot: $slot) -> Self {
                match slot {
                    $slot::Slot0 => Self(uuid::Uuid::nil()),
                    $slot::Slot1 => Self(uuid::Uuid::max()),
                }
            }

            /// 如果当前 ID 对应某个保留槽位，则返回该槽位。
            pub fn slot(self) -> Option<$slot> {
                if self.0 == uuid::Uuid::nil() {
                    Some($slot::Slot0)
                } else if self.0 == uuid::Uuid::max() {
                    Some($slot::Slot1)
                } else {
                    None
                }
            }

            /// 当前 ID 是否属于保留槽位。
            pub fn is_slot(self) -> bool {
                self.slot().is_some()
            }

            /// 从原始 UUID 构造强类型 ID。
            ///
            /// 该方法不会拒绝 nil/max；
            /// 如果传入对应值，之后 `slot()` 会识别为保留槽位。
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
