//! Manifest capability 到插件领域模型的归一化转换。
//!
//! 转换会结合 Manifest runtime 补全 executor，并把面向作者的简洁字段映射成 Host
//! 注册和匹配 Action 所需的完整定义；这里不负责校验整个 Manifest 的跨字段约束。

use super::{
    ActionAppliesTo, ActionRequirements, ContentDelivery, DirectoryActionAppliesToCapability,
    DirectoryActionCapability, DirectoryActionRequirementsCapability, ManifestActionAccess,
    PluginRuntime, ResourceActionCapability,
};
use crate::{
    ActionAccess, ActionDefinitionUi, ActionExecutorKind, ActionOutputContract,
    DirectoryActionAppliesTo, DirectoryActionDefinition, DirectoryActionRequirements,
    ResourceActionAccess, ResourceActionAppliesTo, ResourceActionContentDelivery,
    ResourceActionDefinition, ResourceActionExecutorKind, ResourceActionOutputContract,
    ResourceActionRequirements, ResourceActionUi,
};

impl ResourceActionCapability {
    pub fn to_definition(&self, runtime: &PluginRuntime) -> ResourceActionDefinition {
        let mut definition = ResourceActionDefinition::new(self.id.clone(), self.label.clone())
            .with_handler(self.handler.clone())
            .with_executor(self.executor_kind(runtime))
            .with_access(self.access.to_resource_action_access())
            .with_applies_to(self.applies_to.to_definition())
            .with_output(ResourceActionOutputContract {
                view: self.views.clone(),
            });
        if let Some(requires) = &self.requires {
            definition = definition.with_requirements(requires.to_definition());
        }
        if let Some(ui) = &self.ui {
            definition = definition.with_ui(ResourceActionUi {
                group: ui.group.clone(),
                order: ui.order,
                locations: ui.locations.clone(),
            });
        }
        definition
    }

    pub fn executor_kind(&self, runtime: &PluginRuntime) -> ResourceActionExecutorKind {
        match runtime {
            PluginRuntime::Builtin => ResourceActionExecutorKind::Builtin,
            PluginRuntime::Extism { .. } => ResourceActionExecutorKind::Plugin,
        }
    }

    pub fn handler(&self) -> &str {
        self.handler.as_str()
    }
}

impl DirectoryActionCapability {
    pub fn to_definition(&self, runtime: &PluginRuntime) -> DirectoryActionDefinition {
        let mut definition = DirectoryActionDefinition::new(self.id.clone(), self.label.clone())
            .with_handler(self.handler.clone())
            .with_executor(match runtime {
                PluginRuntime::Builtin => ActionExecutorKind::Builtin,
                PluginRuntime::Extism { .. } => ActionExecutorKind::Plugin,
            })
            .with_access(match self.access {
                ManifestActionAccess::Read => ActionAccess::ReadOnly,
                ManifestActionAccess::Write => ActionAccess::ReadWrite,
            })
            .with_applies_to(self.applies_to.to_definition())
            .with_output(ActionOutputContract {
                view: self.views.clone(),
            });
        if let Some(requires) = &self.requires {
            definition = definition.with_requirements(requires.to_definition());
        }
        if let Some(ui) = &self.ui {
            definition = definition.with_ui(ActionDefinitionUi {
                group: ui.group.clone(),
                order: ui.order,
                locations: ui.locations.clone(),
            });
        }
        definition
    }

    pub fn handler(&self) -> &str {
        &self.handler
    }
}

impl DirectoryActionAppliesToCapability {
    pub fn to_definition(&self) -> DirectoryActionAppliesTo {
        DirectoryActionAppliesTo::new().with_kinds(self.kinds.clone())
    }
}

impl DirectoryActionRequirementsCapability {
    pub fn to_definition(&self) -> DirectoryActionRequirements {
        DirectoryActionRequirements {
            children: self.children,
            resources: self.resources,
        }
    }
}

impl ManifestActionAccess {
    pub fn to_resource_action_access(self) -> ResourceActionAccess {
        match self {
            Self::Read => ResourceActionAccess::ReadOnly,
            Self::Write => ResourceActionAccess::ReadWrite,
        }
    }
}

impl ActionAppliesTo {
    pub fn to_definition(&self) -> ResourceActionAppliesTo {
        ResourceActionAppliesTo::new()
            .with_kinds(self.kinds.clone())
            .with_mime_types(self.media_types.clone())
            .with_extensions(self.extensions.clone())
    }
}

impl ActionRequirements {
    pub fn to_definition(&self) -> ResourceActionRequirements {
        ResourceActionRequirements {
            content: self.content,
            content_delivery: self
                .content_delivery
                .map(ContentDelivery::to_resource_delivery)
                .unwrap_or(ResourceActionContentDelivery::Auto),
        }
    }
}

impl ContentDelivery {
    pub fn to_resource_delivery(self) -> ResourceActionContentDelivery {
        match self {
            Self::Inline => ResourceActionContentDelivery::Inline,
            Self::Reference => ResourceActionContentDelivery::Reference,
        }
    }
}
