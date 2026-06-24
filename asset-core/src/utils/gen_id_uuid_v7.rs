/// 生成基于 UUID v7 的强类型 ID。
///
/// 防止不同业务类型的 ID 被混用，同时保留 UUID v7 的时间有序特性。
#[macro_export]
macro_rules! gen_id_uuid_v7 {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        pub struct $name(pub uuid::Uuid);

        impl $name {
            /// 基于 UUID v7 算法创建一个时间有序 ID。
            pub fn new() -> Self {
                Self(uuid::Uuid::now_v7())
            }

            /// 从现有的 `uuid::Uuid` 实例构造。
            pub fn from_uuid(id: uuid::Uuid) -> Self {
                Self(id)
            }

            /// 借用并获取内部的 `uuid::Uuid` 实例。
            pub fn as_uuid(&self) -> uuid::Uuid {
                self.0
            }
        }

        /// 默认实现：通过 `Self::new()` 生成一个唯一 ID。
        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        /// 格式化输出：直接输出 UUID 的标准字符串形式（36字符，带连字符）。
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        /// 支持从 `uuid::Uuid` 无损转换为强类型 ID。
        impl From<uuid::Uuid> for $name {
            fn from(id: uuid::Uuid) -> Self {
                Self(id)
            }
        }

        /// 支持将强类型 ID 解包并转换为原始的 `uuid::Uuid`。
        impl From<$name> for uuid::Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }

        /// 支持将满足 UUID 规范的字符串解析为当前强类型 ID。
        impl std::str::FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(uuid::Uuid::parse_str(s)?))
            }
        }
    };
}
