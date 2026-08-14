use asset_core::{
    CoreError,
    domain::{DirectoryId, DirectoryResourceAccess, ResourceId, StorageKey},
    port::{DirectoryActionRequest, DirectoryQuery, ListResources, ResourceQuery},
};
use asset_plugin_api::manifest::{PluginPermission, PluginPermissions};
use asset_plugin_api::protocol::directory::{
    PluginDirectoryChild, PluginDirectoryPage, PluginDirectoryResource, PluginDirectoryResourcePage,
};
use asset_plugin_api::protocol::{PluginContentReference, PluginContentReferenceEncoding};
use extism::{Function, PTR, UserData};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use super::content_abi::{ContentLease, HostContentResolver, plugin_resource_content};

const MAX_PAGE_SIZE: u32 = 100;

#[derive(Clone)]
pub(super) struct HostDirectoryResolver {
    directories: Arc<dyn DirectoryQuery>,
    resources: Arc<dyn ResourceQuery>,
    content: HostContentResolver,
    permissions: PluginPermissions,
    state: Arc<Mutex<HashMap<String, AvailableDirectory>>>,
    runtime: tokio::runtime::Handle,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageRequest {
    reference: String,
    #[serde(default)]
    cursor: Option<String>,
    limit: u32,
}

#[derive(Clone)]
struct AvailableDirectory {
    id: DirectoryId,
    children: bool,
    resources: DirectoryResourceAccess,
    content_leases: Arc<Mutex<HashMap<ResourceId, ContentLease>>>,
}

extism::host_fn!(asset_hub_directory_list_children(user_data: HostDirectoryResolver; request: String) -> String {
    let resolver = user_data.get()?.lock().map_err(|_| extism::Error::msg("directory host data lock poisoned"))?.clone();
    resolver.list_children(&request).map_err(|error| extism::Error::msg(error.to_string()))
});

extism::host_fn!(asset_hub_directory_list_resources(user_data: HostDirectoryResolver; request: String) -> String {
    let resolver = user_data.get()?.lock().map_err(|_| extism::Error::msg("directory host data lock poisoned"))?.clone();
    resolver.list_resources(&request).map_err(|error| extism::Error::msg(error.to_string()))
});

pub(super) fn host_functions(resolver: &HostDirectoryResolver) -> [Function; 2] {
    [
        Function::new(
            "asset_hub_directory_list_children",
            [PTR],
            [PTR],
            UserData::new(resolver.clone()),
            asset_hub_directory_list_children,
        ),
        Function::new(
            "asset_hub_directory_list_resources",
            [PTR],
            [PTR],
            UserData::new(resolver.clone()),
            asset_hub_directory_list_resources,
        ),
    ]
}

impl HostDirectoryResolver {
    pub(super) fn new(
        directories: Arc<dyn DirectoryQuery>,
        resources: Arc<dyn ResourceQuery>,
        content: HostContentResolver,
        permissions: PluginPermissions,
    ) -> Self {
        Self {
            directories,
            resources,
            content,
            permissions,
            state: Arc::new(Mutex::new(HashMap::new())),
            runtime: tokio::runtime::Handle::current(),
        }
    }

    pub(super) fn register(
        &self,
        request: &DirectoryActionRequest,
    ) -> Result<DirectoryLease, CoreError> {
        let reference = format!("directory:reference:{}", uuid::Uuid::now_v7());
        let content_leases = Arc::new(Mutex::new(HashMap::new()));
        self.state
            .lock()
            .map_err(|_| CoreError::configuration("directory host state lock poisoned"))?
            .insert(
                reference.clone(),
                AvailableDirectory {
                    id: request.directory().id(),
                    children: request.requirements().children,
                    resources: request.requirements().resources,
                    content_leases: content_leases.clone(),
                },
            );
        Ok(DirectoryLease {
            state: self.state.clone(),
            reference,
            content_leases,
        })
    }

    fn available_directory(&self, reference: &str) -> Result<AvailableDirectory, CoreError> {
        self.state
            .lock()
            .map_err(|_| CoreError::configuration("directory host state lock poisoned"))?
            .get(reference)
            .cloned()
            .ok_or_else(|| CoreError::configuration("directory reference is not available"))
    }

    fn page_request(&self, value: &str) -> Result<(PageRequest, u64), CoreError> {
        let request: PageRequest = serde_json::from_str(value).map_err(|error| {
            CoreError::configuration(format!("invalid directory page request: {error}"))
        })?;
        if request.limit == 0 || request.limit > MAX_PAGE_SIZE {
            return Err(CoreError::configuration(format!(
                "directory page limit must be between 1 and {MAX_PAGE_SIZE}"
            )));
        }
        let offset = request
            .cursor
            .as_deref()
            .unwrap_or("0")
            .parse::<u64>()
            .map_err(|_| CoreError::configuration("invalid directory page cursor"))?;
        Ok((request, offset))
    }

    fn list_children(&self, value: &str) -> Result<String, CoreError> {
        if !self
            .permissions
            .allows(PluginPermission::DirectoryChildrenList)
        {
            return Err(CoreError::configuration(
                "plugin lacks directory.children.list permission",
            ));
        }
        let (request, offset) = self.page_request(value)?;
        let available = self.available_directory(&request.reference)?;
        if !available.children {
            return Err(CoreError::configuration(
                "directory action did not declare a children requirement",
            ));
        }
        let directory_id = available.id;
        let mut items = self
            .runtime
            .block_on(self.directories.list_children(&directory_id))?;
        items.sort_by(|left, right| left.directory().name().cmp(right.directory().name()));
        let total = items.len() as u64;
        let page = items
            .into_iter()
            .skip(offset as usize)
            .take(request.limit as usize)
            .map(|located| PluginDirectoryChild {
                id: located.id().to_string(),
                name: located.directory().name().to_string(),
                path: located.path().path().to_string(),
                kind: located.directory().kind().as_str().to_string(),
            })
            .collect();
        serde_json::to_string(&PluginDirectoryPage {
            items: page,
            next_cursor: (offset + u64::from(request.limit) < total)
                .then(|| (offset + u64::from(request.limit)).to_string()),
        })
        .map_err(|error| CoreError::configuration(error.to_string()))
    }

    fn list_resources(&self, value: &str) -> Result<String, CoreError> {
        if !self
            .permissions
            .allows(PluginPermission::DirectoryResourcesList)
        {
            return Err(CoreError::configuration(
                "plugin lacks directory.resources.list permission",
            ));
        }
        let (request, offset) = self.page_request(value)?;
        let available = self.available_directory(&request.reference)?;
        if !available.resources.includes_metadata() {
            return Err(CoreError::configuration(
                "directory action did not declare a resources requirement",
            ));
        }
        if available.resources.includes_content()
            && (!self.permissions.resource_read() || !self.permissions.resource_content_read())
        {
            return Err(CoreError::configuration(
                "directory resource content requires resource.read and resource.content.read permissions",
            ));
        }
        let directory_id = available.id;
        let page =
            self.runtime.block_on(self.resources.list(
                &ListResources::new(request.limit, offset).with_directory_id(directory_id),
            ))?;
        serde_json::to_string(&PluginDirectoryResourcePage {
            items: page
                .items
                .into_iter()
                .map(|located| -> Result<PluginDirectoryResource, CoreError> {
                    let resource = located.resource();
                    let content_ref = if available.resources.includes_content() {
                        let existing_reference = {
                            let leases = available.content_leases.lock().map_err(|_| {
                                CoreError::configuration(
                                    "directory content lease map lock poisoned",
                                )
                            })?;
                            leases
                                .get(&resource.id())
                                .map(|lease| lease.reference().to_string())
                        };
                        if let Some(reference) = existing_reference {
                            Some(reference)
                        } else if let Some(content) = resource.content() {
                            let key = StorageKey::from_resource_path(
                                located.directory().path(),
                                resource.name(),
                            )?;
                            let Some(lease) =
                                self.content.register_available(key, content.size())?
                            else {
                                return Ok(PluginDirectoryResource {
                                    id: resource.id().to_string(),
                                    name: resource.name().to_string(),
                                    kind: resource.kind().as_str().to_string(),
                                    revision: resource.revision(),
                                    content: resource.content().map(plugin_resource_content),
                                    content_ref: None,
                                });
                            };
                            let reference = lease.reference().to_string();
                            available
                                .content_leases
                                .lock()
                                .map_err(|_| {
                                    CoreError::configuration(
                                        "directory content lease map lock poisoned",
                                    )
                                })?
                                .insert(resource.id(), lease);
                            Some(reference)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    Ok(PluginDirectoryResource {
                        id: resource.id().to_string(),
                        name: resource.name().to_string(),
                        kind: resource.kind().as_str().to_string(),
                        revision: resource.revision(),
                        content: resource.content().map(plugin_resource_content),
                        content_ref: content_ref.map(|reference| PluginContentReference {
                            encoding: PluginContentReferenceEncoding::Handle,
                            reference,
                        }),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            next_cursor: (offset + u64::from(page.limit) < page.total)
                .then(|| (offset + u64::from(page.limit)).to_string()),
        })
        .map_err(|error| CoreError::configuration(error.to_string()))
    }
}

pub(super) struct DirectoryLease {
    state: Arc<Mutex<HashMap<String, AvailableDirectory>>>,
    reference: String,
    content_leases: Arc<Mutex<HashMap<ResourceId, ContentLease>>>,
}

impl DirectoryLease {
    pub(super) fn reference(&self) -> &str {
        &self.reference
    }
}

impl Drop for DirectoryLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.remove(&self.reference);
        }
        if let Ok(mut leases) = self.content_leases.lock() {
            leases.clear();
        }
    }
}
