use asset_core::CoreError;
use asset_core::domain::StorageKey;
use asset_core::port::{BlobStorage, ResourceActionRequest};
use asset_plugin_api::{
    PluginActionRequest, PluginChecksum, PluginContentBytes, PluginContentRange,
    PluginContentReference, PluginContentReferenceEncoding, PluginExecutionPolicy,
    PluginInlineContentEncoding, PluginPermissions, PluginResource, PluginResourceContent,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use extism::{CompiledPlugin, Function, PTR, PluginBuilder, UserData};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::permissions::manifest_for_plugin;

#[derive(Clone)]
pub(super) struct HostContentResolver {
    pub(super) storage: Arc<dyn BlobStorage>,
    pub(super) state: Arc<Mutex<HostContentState>>,
    pub(super) runtime: tokio::runtime::Handle,
    pub(super) policy: Arc<PluginExecutionPolicy>,
}

#[derive(Default)]
pub(super) struct HostContentState {
    pub(super) references: HashMap<String, AvailableContent>,
    pub(super) handles: HashMap<String, OpenContent>,
}

#[derive(Clone)]
pub(super) struct AvailableContent {
    pub(super) key: StorageKey,
    pub(super) size: u64,
}

#[derive(Clone)]
pub(super) struct OpenContent {
    pub(super) reference: String,
    pub(super) key: StorageKey,
    pub(super) size: u64,
}

extism::host_fn!(asset_hub_content_open(user_data: HostContentResolver; reference: String) -> String {
    let content = user_data.get()?;
    let content = content
        .lock()
        .map_err(|_| extism::Error::msg("content host data lock poisoned"))?
        .clone();
    content.open(&reference).map_err(|error| extism::Error::msg(error.to_string()))
});

extism::host_fn!(asset_hub_content_size(user_data: HostContentResolver; handle: String) -> u64 {
    let content = user_data.get()?;
    let content = content
        .lock()
        .map_err(|_| extism::Error::msg("content host data lock poisoned"))?
        .clone();
    content.size(&handle).map_err(|error| extism::Error::msg(error.to_string()))
});

extism::host_fn!(asset_hub_content_read(user_data: HostContentResolver; handle: String, offset: u64, length: u64) -> Vec<u8> {
    let content = user_data.get()?;
    let content = content
        .lock()
        .map_err(|_| extism::Error::msg("content host data lock poisoned"))?
        .clone();
    content
        .read(&handle, offset, length)
        .map_err(|error| extism::Error::msg(error.to_string()))
});

extism::host_fn!(asset_hub_content_close(user_data: HostContentResolver; handle: String) {
    let content = user_data.get()?;
    let content = content
        .lock()
        .map_err(|_| extism::Error::msg("content host data lock poisoned"))?
        .clone();
    content.close(&handle).map_err(|error| extism::Error::msg(error.to_string()))?;
    Ok(())
});

pub(super) fn compile_plugin(
    plugin_id: &str,
    wasm: &[u8],
    wasi: bool,
    permissions: &PluginPermissions,
    host_content: &HostContentResolver,
    policy: &PluginExecutionPolicy,
) -> Result<Arc<CompiledPlugin>, CoreError> {
    let content_open = Function::new(
        "asset_hub_content_open",
        [PTR],
        [PTR],
        UserData::new(host_content.clone()),
        asset_hub_content_open,
    );
    let content_size = Function::new(
        "asset_hub_content_size",
        [PTR],
        [PTR],
        UserData::new(host_content.clone()),
        asset_hub_content_size,
    );
    let content_read = Function::new(
        "asset_hub_content_read",
        [PTR, PTR, PTR],
        [PTR],
        UserData::new(host_content.clone()),
        asset_hub_content_read,
    );
    let content_close = Function::new(
        "asset_hub_content_close",
        [PTR],
        [],
        UserData::new(host_content.clone()),
        asset_hub_content_close,
    );
    PluginBuilder::new(manifest_for_plugin(wasm, permissions, policy))
        .with_wasi(wasi)
        .with_functions([content_open, content_size, content_read, content_close])
        .compile()
        .map(Arc::new)
        .map_err(|error| {
            CoreError::configuration(format!(
                "compile plugin `{plugin_id}` verified Wasm: {error}"
            ))
        })
}

impl HostContentResolver {
    pub(super) fn register(
        &self,
        plugin_id: &str,
        request: &ResourceActionRequest,
    ) -> Result<Option<ContentLease>, CoreError> {
        if !matches!(
            request.content_delivery(),
            asset_plugin_api::ResourceActionContentDelivery::Reference
        ) {
            return Ok(None);
        }
        let Some(content) = request.resource().content() else {
            return Ok(None);
        };
        if content.size() > self.policy.max_content_bytes() {
            return Err(CoreError::plugin(
                plugin_id,
                request.action().as_str(),
                format!(
                    "resource content is {} bytes, plugin limit is {}",
                    content.size(),
                    self.policy.max_content_bytes()
                ),
            ));
        }
        let reference = content_reference();
        self.state
            .lock()
            .map_err(|_| CoreError::configuration("content reference map lock poisoned"))?
            .references
            .insert(
                reference.clone(),
                AvailableContent {
                    key: request.resource().storage_key(),
                    size: content.size(),
                },
            );
        Ok(Some(ContentLease {
            state: self.state.clone(),
            reference,
        }))
    }

    pub(super) fn open(&self, reference: &str) -> Result<String, CoreError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| CoreError::configuration("content host state lock poisoned"))?;
        let content = state.references.get(reference).cloned().ok_or_else(|| {
            CoreError::configuration(format!("content reference `{reference}` is not available"))
        })?;
        let handle = format!("content:handle:{}", uuid::Uuid::now_v7());
        state.handles.insert(
            handle.clone(),
            OpenContent {
                reference: reference.to_string(),
                key: content.key,
                size: content.size,
            },
        );
        Ok(handle)
    }

    pub(super) fn size(&self, handle: &str) -> Result<u64, CoreError> {
        self.open_content(handle).map(|content| content.size)
    }

    pub(super) fn read(
        &self,
        handle: &str,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, CoreError> {
        let content = self.open_content(handle)?;
        let range = PluginContentRange::new(offset, length)
            .and_then(|range| range.bounded(content.size, self.policy.max_content_read_bytes()))
            .map_err(|error| CoreError::configuration(error.to_string()))?;
        if range.length() == 0 {
            return Ok(Vec::new());
        }
        let offset = range.offset();
        let length = range.length();
        let end = range.end() - 1;
        self.runtime.block_on(async {
            let Some(mut stream) = self
                .storage
                .get_range_stream(&content.key, offset, end)
                .await?
            else {
                return Err(CoreError::not_found(
                    "plugin content",
                    content.key.to_string(),
                ));
            };
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                bytes.extend_from_slice(&chunk?);
                if bytes.len() as u64 > length {
                    return Err(CoreError::configuration(
                        "content storage returned more bytes than requested",
                    ));
                }
            }
            Ok(bytes)
        })
    }

    pub(super) fn close(&self, handle: &str) -> Result<(), CoreError> {
        let removed = self
            .state
            .lock()
            .map_err(|_| CoreError::configuration("content host state lock poisoned"))?
            .handles
            .remove(handle);
        if removed.is_none() {
            return Err(CoreError::configuration(format!(
                "content handle `{handle}` is not open"
            )));
        }
        Ok(())
    }

    pub(super) fn open_content(&self, handle: &str) -> Result<OpenContent, CoreError> {
        self.state
            .lock()
            .map_err(|_| CoreError::configuration("content host state lock poisoned"))?
            .handles
            .get(handle)
            .cloned()
            .ok_or_else(|| {
                CoreError::configuration(format!("content handle `{handle}` is not open"))
            })
    }
}

pub(super) struct ContentLease {
    pub(super) state: Arc<Mutex<HostContentState>>,
    pub(super) reference: String,
}

impl ContentLease {
    pub(super) fn reference(&self) -> &str {
        &self.reference
    }
}

impl Drop for ContentLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.references.remove(&self.reference);
            state
                .handles
                .retain(|_, handle| handle.reference != self.reference);
        }
    }
}

pub(super) fn build_payload(
    request: &ResourceActionRequest,
    content_reference: Option<&str>,
) -> PluginActionRequest {
    let resource = request.resource();
    let content_ref = resource.content();
    let content = if matches!(
        request.content_delivery(),
        asset_plugin_api::ResourceActionContentDelivery::Reference
    ) {
        None
    } else {
        request.content().map(|content| PluginContentBytes {
            encoding: PluginInlineContentEncoding::Base64,
            data: STANDARD.encode(content),
        })
    };
    let content_ref_payload = if matches!(
        request.content_delivery(),
        asset_plugin_api::ResourceActionContentDelivery::Reference
    ) {
        content_ref.map(|_| PluginContentReference {
            abi_version: asset_plugin_api::CONTENT_ABI_VERSION,
            encoding: PluginContentReferenceEncoding::Handle,
            reference: content_reference
                .expect("reference content delivery must hold a content lease")
                .to_string(),
        })
    } else {
        None
    };

    PluginActionRequest {
        action: request.action().as_str().to_string(),
        access: request.access(),
        input: request.input().clone(),
        resource: PluginResource {
            id: resource.id().to_string(),
            directory: resource.directory().path().to_string(),
            name: resource.name().to_string(),
            kind: resource.kind().as_str().to_string(),
            status: resource.status().as_str().to_string(),
            tags: resource
                .tags()
                .iter()
                .map(|tag| tag.as_str().to_owned())
                .collect(),
            content: content_ref.map(|content| PluginResourceContent {
                size: content.size(),
                mime_type: content.mime_type().map(str::to_string),
                checksum: PluginChecksum {
                    kind: content.checksum().kind().as_str().to_string(),
                    value: content.checksum().value().to_string(),
                },
            }),
            created_at: resource.created_at().to_rfc3339(),
            updated_at: resource.updated_at().to_rfc3339(),
            deleted_at: resource.deleted_at().map(|value| value.to_rfc3339()),
        },
        content,
        content_ref: content_ref_payload,
    }
}

pub(super) fn content_reference() -> String {
    format!("asset://content/{}", uuid::Uuid::now_v7())
}
