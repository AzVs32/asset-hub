//! 外部模块声明自有配置分区时实现的公共约定。

use serde::{Serialize, de::DeserializeOwned};

/// 共享配置文档中由一个模块独立拥有的强类型配置分区。
///
/// 分区名使用 `asset`、`http` 或 `plugins.example` 这样的点分路径。分区缺失时使用
/// [`Default`]；配置进入 [`crate::LoadedConfig`] 前，模块可以通过
/// [`ConfigSection::normalize`] 完成归一化和语义校验。
pub trait ConfigSection:
    Default + DeserializeOwned + Serialize + Send + Sync + Sized + 'static
{
    const SECTION: &'static str;

    fn normalize(self) -> Result<Self, String> {
        Ok(self)
    }
}
