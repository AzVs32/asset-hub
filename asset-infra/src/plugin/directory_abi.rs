use asset_core::{
    CoreError,
    domain::DirectoryId,
    port::{DirectoryActionRequest, DirectoryQuery, ListResources, ResourceQuery},
};
use asset_plugin_api::protocol::directory::{
    PluginDirectoryChild, PluginDirectoryPage, PluginDirectoryResource, PluginDirectoryResourcePage,
};
use asset_plugin_api::{PluginPermission, PluginPermissions};
use extism::{Function, PTR, UserData};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

const MAX_PAGE_SIZE: u32 = 100;

#[derive(Clone)]
pub(super) struct HostDirectoryResolver {
    directories: Arc<dyn DirectoryQuery>,
    resources: Arc<dyn ResourceQuery>,
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

#[derive(Debug, Clone, Copy)]
struct AvailableDirectory {
    id: DirectoryId,
    children: bool,
    resources: bool,
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
        permissions: PluginPermissions,
    ) -> Self {
        Self {
            directories,
            resources,
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
        self.state
            .lock()
            .map_err(|_| CoreError::configuration("directory host state lock poisoned"))?
            .insert(
                reference.clone(),
                AvailableDirectory {
                    id: request.directory().id(),
                    children: request.requirements().children,
                    resources: request.requirements().resources,
                },
            );
        Ok(DirectoryLease {
            state: self.state.clone(),
            reference,
        })
    }

    fn available_directory(&self, reference: &str) -> Result<AvailableDirectory, CoreError> {
        self.state
            .lock()
            .map_err(|_| CoreError::configuration("directory host state lock poisoned"))?
            .get(reference)
            .copied()
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
        if !available.resources {
            return Err(CoreError::configuration(
                "directory action did not declare a resources requirement",
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
                .map(|located| {
                    let resource = located.resource();
                    PluginDirectoryResource {
                        id: resource.id().to_string(),
                        name: resource.name().to_string(),
                        kind: resource.kind().as_str().to_string(),
                    }
                })
                .collect(),
            next_cursor: (offset + u64::from(page.limit) < page.total)
                .then(|| (offset + u64::from(page.limit)).to_string()),
        })
        .map_err(|error| CoreError::configuration(error.to_string()))
    }
}

pub(super) struct DirectoryLease {
    state: Arc<Mutex<HashMap<String, AvailableDirectory>>>,
    reference: String,
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
    }
}
