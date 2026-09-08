use super::{ContentVerificationStatus, Resource};

/// Resource 聚合的生命周期状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceLifecycleStatus {
    Active,
}

/// 面向后端策略的单值有效状态。
///
/// 该状态不持久化，也不能作为状态转换命令的输入。它只为需要单值判断的消费者提供统一
/// 优先级：根据内容引用和校验结果派生。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceEffectiveStatus {
    NoContent,
    Verifying,
    Ready,
    VerificationFailed,
}

/// Resource 生命周期与内容状态的只读统一投影。
///
/// 投影保留两个正交状态轴；需要单值判断时使用 [`Self::effective`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceState {
    lifecycle: ResourceLifecycleStatus,
    content: Option<ContentVerificationStatus>,
}

impl ResourceState {
    pub(super) fn from_resource(resource: &Resource) -> Self {
        let lifecycle = ResourceLifecycleStatus::Active;
        let content = resource
            .content()
            .map(|content| content.verification_status());

        Self { lifecycle, content }
    }

    /// 返回聚合生命周期状态。
    pub const fn lifecycle(self) -> ResourceLifecycleStatus {
        self.lifecycle
    }

    /// 返回内容引用的校验状态；没有对象内容引用时返回 `None`。
    pub const fn content(self) -> Option<ContentVerificationStatus> {
        self.content
    }

    /// 按 Core 的统一优先级返回单值有效状态。
    pub const fn effective(self) -> ResourceEffectiveStatus {
        match self.content {
            None => ResourceEffectiveStatus::NoContent,
            Some(ContentVerificationStatus::Pending) => ResourceEffectiveStatus::Verifying,
            Some(ContentVerificationStatus::Verified) => ResourceEffectiveStatus::Ready,
            Some(ContentVerificationStatus::Failed) => ResourceEffectiveStatus::VerificationFailed,
        }
    }
}
