//! High-level authoring facade for Extism/Wasm plugin Actions.
//!
//! This module deliberately owns wire decoding, structured failures, content handles, pagination,
//! protocol-version fields, and base64 representations so normal plugin code can express intent.

#[cfg(target_arch = "wasm32")]
use crate::abi;
use crate::protocol::{
    CreateChildDirectoryEffect, CreateDirectoryTreeEffect, CreateTreeDirectory, CreateTreeResource,
    CreateTreeResourceEncoding, DirectoryActionEffect, DownloadView, HtmlView, JsonView,
    MarkdownView, MediaView, PLUGIN_API_VERSION, PluginActionFailure,
    PluginContentReferenceEncoding, PluginDiagnostic, PluginDirectory, PluginDirectoryActionOutput,
    PluginDirectoryActionRequest, PluginDirectoryChild, PluginDirectoryResource, PluginFrameView,
    PluginInlineContentEncoding, PluginMediaEncoding, PluginReplacementEncoding, PluginResource,
    PluginResourceActionEffect, PluginResourceActionOutput, PluginResourceActionRequest,
    PluginView, ReplaceContentEffect, TextView, UpdateDirectoryEffect,
};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
pub use extism_pdk::Error;
use extism_pdk::FnResult;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Result type used by plugin business handlers.
pub type Result<T> = FnResult<T>;

/// Encodes bytes using the canonical base64 representation used by Plugin API payloads.
pub fn encode_base64(bytes: impl AsRef<[u8]>) -> String {
    STANDARD.encode(bytes.as_ref())
}

/// Encodes bytes for compact URL-fragment payloads used by plugin frames.
pub fn encode_base64_url(bytes: impl AsRef<[u8]>) -> String {
    URL_SAFE_NO_PAD.encode(bytes.as_ref())
}

/// Decodes the canonical base64 representation used by Plugin API payloads.
pub fn decode_base64(value: &str) -> Result<Vec<u8>> {
    Ok(STANDARD.decode(value)?)
}

/// Invokes a high-level Resource Action and always serializes plugin failures structurally.
pub fn run_resource_action(
    input: String,
    handler: impl FnOnce(ResourceContext) -> Result<ResourceResponse>,
) -> FnResult<String> {
    structured_action_result((|| {
        let request = serde_json::from_str(&input)?;
        let output = handler(ResourceContext { request })?;
        Ok(serde_json::to_string(&output.inner)?)
    })())
}

/// Invokes a high-level Directory Action and always serializes plugin failures structurally.
pub fn run_directory_action(
    input: String,
    handler: impl FnOnce(DirectoryContext) -> Result<DirectoryResponse>,
) -> FnResult<String> {
    structured_action_result((|| {
        let request = serde_json::from_str(&input)?;
        let output = handler(DirectoryContext { request })?;
        Ok(serde_json::to_string(&output.inner)?)
    })())
}

fn structured_action_result(result: FnResult<String>) -> FnResult<String> {
    match result {
        Ok(output) => Ok(output),
        Err(error) => Ok(serde_json::to_string(&PluginActionFailure::new(
            PluginDiagnostic::error(
                crate::protocol::diagnostic::codes::ACTION_FAILED,
                error.0.to_string(),
            ),
        ))?),
    }
}

/// Resource Action invocation context.
pub struct ResourceContext {
    request: PluginResourceActionRequest,
}

impl ResourceContext {
    pub fn action(&self) -> &str {
        &self.request.action
    }

    pub fn input(&self) -> &Value {
        &self.request.input
    }

    pub fn input_as<T: DeserializeOwned>(&self) -> Result<T> {
        Ok(serde_json::from_value(self.request.input.clone())?)
    }

    pub fn resource(&self) -> ResourceSnapshot<'_> {
        ResourceSnapshot(&self.request.resource)
    }

    pub fn content(&self) -> ResourceContent<'_> {
        ResourceContent { context: self }
    }
}

/// Read-only Resource snapshot exposed to authoring code.
#[derive(Clone, Copy)]
pub struct ResourceSnapshot<'a>(&'a PluginResource);

impl<'a> ResourceSnapshot<'a> {
    pub fn id(self) -> &'a str {
        &self.0.id
    }

    pub fn directory(self) -> &'a str {
        &self.0.directory
    }

    pub fn name(self) -> &'a str {
        &self.0.name
    }

    pub fn kind(self) -> &'a str {
        &self.0.kind
    }

    pub fn revision(self) -> u64 {
        self.0.revision
    }

    pub fn updated_at(self) -> &'a str {
        &self.0.updated_at
    }

    pub fn content_size(self) -> Option<u64> {
        self.0.content.as_ref().map(|content| content.size)
    }

    pub fn mime_type(self) -> Option<&'a str> {
        self.0
            .content
            .as_ref()
            .and_then(|content| content.mime_type.as_deref())
    }

    pub fn checksum(self) -> Option<(&'a str, &'a str)> {
        self.0
            .content
            .as_ref()
            .and_then(|content| content.checksum.as_ref())
            .map(|checksum| (checksum.kind.as_str(), checksum.value.as_str()))
    }
}

/// Unified inline/reference content reader for the current Resource Action.
pub struct ResourceContent<'a> {
    context: &'a ResourceContext,
}

impl ResourceContent<'_> {
    pub fn is_available(&self) -> bool {
        self.context.request.content.is_some() || self.context.request.content_ref.is_some()
    }

    pub fn size(&self) -> Result<u64> {
        if let Some(content) = &self.context.request.content {
            ensure_inline_encoding(content.encoding)?;
            return Ok(decode_base64(&content.data)?.len() as u64);
        }
        self.context
            .request
            .resource
            .content
            .as_ref()
            .map(|content| content.size)
            .ok_or_else(|| Error::msg("missing resource content description").into())
    }

    pub fn read_all(&self, max_size: u64, chunk_size: u64) -> Result<Vec<u8>> {
        if let Some(content) = &self.context.request.content {
            ensure_inline_encoding(content.encoding)?;
            let bytes = decode_base64(&content.data)?;
            ensure_content_limit(bytes.len() as u64, max_size)?;
            return Ok(bytes);
        }
        let size = self.size()?;
        ensure_content_limit(size, max_size)?;
        self.read_range(0, size, max_size, chunk_size)
    }

    pub fn read_range(
        &self,
        offset: u64,
        length: u64,
        max_size: u64,
        chunk_size: u64,
    ) -> Result<Vec<u8>> {
        if chunk_size == 0 {
            return Err(Error::msg("content chunk size must be greater than zero").into());
        }
        if let Some(content) = &self.context.request.content {
            ensure_inline_encoding(content.encoding)?;
            let bytes = decode_base64(&content.data)?;
            let size = bytes.len() as u64;
            ensure_content_limit(size, max_size)?;
            let end = checked_content_end(offset, length, size)?;
            let start = usize::try_from(offset)
                .map_err(|_| Error::msg("content offset does not fit guest memory"))?;
            let end = usize::try_from(end)
                .map_err(|_| Error::msg("content end does not fit guest memory"))?;
            return Ok(bytes[start..end].to_vec());
        }

        let size = self.size()?;
        ensure_content_limit(size, max_size)?;
        checked_content_end(offset, length, size)?;

        let reference = self
            .context
            .request
            .content_ref
            .as_ref()
            .ok_or_else(|| Error::msg("missing resource content payload"))?;
        if reference.encoding != PluginContentReferenceEncoding::Handle {
            return Err(Error::msg("unsupported content reference encoding").into());
        }
        read_content_reference(&reference.reference, offset, length, max_size, chunk_size)
    }
}

fn ensure_content_limit(size: u64, max_size: u64) -> Result<()> {
    if size > max_size {
        return Err(Error::msg(format!(
            "content is {size} bytes, plugin limit is {max_size}"
        ))
        .into());
    }
    Ok(())
}

fn checked_content_end(offset: u64, length: u64, size: u64) -> Result<u64> {
    offset
        .checked_add(length)
        .filter(|end| *end <= size)
        .ok_or_else(|| Error::msg("content range is out of bounds").into())
}

fn ensure_inline_encoding(encoding: PluginInlineContentEncoding) -> Result<()> {
    if encoding != PluginInlineContentEncoding::Base64 {
        return Err(Error::msg("unsupported inline content encoding").into());
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn read_content_reference(
    reference: &str,
    offset: u64,
    length: u64,
    max_size: u64,
    chunk_size: u64,
) -> Result<Vec<u8>> {
    let range = abi::content::PluginContentRange::new(offset, length)?;
    abi::content::guest::read_range(reference, range, max_size, chunk_size)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_content_reference(
    _reference: &str,
    _offset: u64,
    _length: u64,
    _max_size: u64,
    _chunk_size: u64,
) -> Result<Vec<u8>> {
    Err(Error::msg("content references are only available in the Wasm Host").into())
}

/// Directory Action invocation context.
pub struct DirectoryContext {
    request: PluginDirectoryActionRequest,
}

impl DirectoryContext {
    pub fn action(&self) -> &str {
        &self.request.action
    }

    pub fn input(&self) -> &Value {
        &self.request.input
    }

    pub fn input_as<T: DeserializeOwned>(&self) -> Result<T> {
        Ok(serde_json::from_value(self.request.input.clone())?)
    }

    pub fn directory(&self) -> DirectorySnapshot<'_> {
        DirectorySnapshot(&self.request.directory)
    }

    /// Collects all direct children, failing instead of silently truncating at `max_items`.
    pub fn children_bounded(&self, max_items: usize) -> Result<Vec<DirectoryChild>> {
        collect_children(&self.request.directory_ref, None, max_items)
    }

    /// Collects direct children of the current Directory or one descendant.
    pub fn children_bounded_in(
        &self,
        directory_id: Option<&str>,
        max_items: usize,
    ) -> Result<Vec<DirectoryChild>> {
        collect_children(&self.request.directory_ref, directory_id, max_items)
    }

    /// Collects Resources in the current Directory or one descendant, with an explicit bound.
    pub fn resources_bounded(
        &self,
        directory_id: Option<&str>,
        max_items: usize,
    ) -> Result<Vec<DirectoryResource>> {
        collect_resources(&self.request.directory_ref, directory_id, max_items)
    }
}

#[derive(Clone, Copy)]
pub struct DirectorySnapshot<'a>(&'a PluginDirectory);

impl<'a> DirectorySnapshot<'a> {
    pub fn id(self) -> &'a str {
        &self.0.id
    }

    pub fn parent_id(self) -> Option<&'a str> {
        self.0.parent_id.as_deref()
    }

    pub fn path(self) -> &'a str {
        &self.0.path
    }

    pub fn name(self) -> &'a str {
        &self.0.name
    }

    pub fn kind(self) -> &'a str {
        &self.0.kind
    }

    pub fn revision(self) -> u64 {
        self.0.revision
    }
}

pub struct DirectoryChild(PluginDirectoryChild);

impl DirectoryChild {
    pub fn id(&self) -> &str {
        &self.0.id
    }

    pub fn name(&self) -> &str {
        &self.0.name
    }

    pub fn path(&self) -> &str {
        &self.0.path
    }

    pub fn kind(&self) -> &str {
        &self.0.kind
    }
}

pub struct DirectoryResource(PluginDirectoryResource);

impl DirectoryResource {
    pub fn id(&self) -> &str {
        &self.0.id
    }

    pub fn name(&self) -> &str {
        &self.0.name
    }

    pub fn kind(&self) -> &str {
        &self.0.kind
    }

    pub fn revision(&self) -> u64 {
        self.0.revision
    }

    pub fn content_size(&self) -> Option<u64> {
        self.0.content.as_ref().map(|content| content.size)
    }

    pub fn mime_type(&self) -> Option<&str> {
        self.0
            .content
            .as_ref()
            .and_then(|content| content.mime_type.as_deref())
    }

    pub fn read_bytes(&self, max_size: u64, chunk_size: u64) -> Result<Option<Vec<u8>>> {
        let Some(reference) = self.0.content_ref.as_ref() else {
            return Ok(None);
        };
        if reference.encoding != PluginContentReferenceEncoding::Handle {
            return Err(Error::msg("unsupported directory resource content reference").into());
        }
        read_all_reference(&reference.reference, max_size, chunk_size).map(Some)
    }

    pub fn read_text(&self, max_size: u64, chunk_size: u64) -> Result<Option<String>> {
        let Some(bytes) = self.read_bytes(max_size, chunk_size)? else {
            return Ok(None);
        };
        Ok(Some(String::from_utf8(bytes).map_err(|_| {
            Error::msg(format!("{} is not valid UTF-8", self.name()))
        })?))
    }
}

#[cfg(target_arch = "wasm32")]
fn collect_children(
    reference: &str,
    directory_id: Option<&str>,
    max_items: usize,
) -> Result<Vec<DirectoryChild>> {
    if max_items == 0 {
        return Err(Error::msg("directory child limit must be greater than zero").into());
    }
    let mut items = Vec::new();
    let mut cursor = None;
    loop {
        let remaining = max_items - items.len();
        let page = abi::directory::guest::list_children_in(
            reference,
            directory_id,
            cursor.as_deref(),
            u32::try_from(remaining.min(100)).unwrap_or(100),
        )?;
        if page.items.is_empty() && page.next_cursor.is_some() {
            return Err(Error::msg("directory Host returned a non-progressing child page").into());
        }
        items.extend(page.items.into_iter().map(DirectoryChild));
        if items.len() > max_items {
            return Err(Error::msg("directory Host exceeded the requested child page size").into());
        }
        match page.next_cursor {
            Some(_) if items.len() >= max_items => {
                return Err(Error::msg(format!(
                    "directory contains more than the plugin limit of {max_items} children"
                ))
                .into());
            }
            Some(next) => cursor = Some(next),
            None => return Ok(items),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn collect_children(
    _reference: &str,
    _directory_id: Option<&str>,
    _max_items: usize,
) -> Result<Vec<DirectoryChild>> {
    Err(Error::msg("directory queries are only available in the Wasm Host").into())
}

#[cfg(target_arch = "wasm32")]
fn collect_resources(
    reference: &str,
    directory_id: Option<&str>,
    max_items: usize,
) -> Result<Vec<DirectoryResource>> {
    if max_items == 0 {
        return Err(Error::msg("directory resource limit must be greater than zero").into());
    }
    let mut items = Vec::new();
    let mut cursor = None;
    loop {
        let remaining = max_items - items.len();
        let page = abi::directory::guest::list_resources_in(
            reference,
            directory_id,
            cursor.as_deref(),
            u32::try_from(remaining.min(100)).unwrap_or(100),
        )?;
        if page.items.is_empty() && page.next_cursor.is_some() {
            return Err(
                Error::msg("directory Host returned a non-progressing resource page").into(),
            );
        }
        items.extend(page.items.into_iter().map(DirectoryResource));
        if items.len() > max_items {
            return Err(
                Error::msg("directory Host exceeded the requested resource page size").into(),
            );
        }
        match page.next_cursor {
            Some(_) if items.len() >= max_items => {
                return Err(Error::msg(format!(
                    "directory contains more than the plugin limit of {max_items} resources"
                ))
                .into());
            }
            Some(next) => cursor = Some(next),
            None => return Ok(items),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn collect_resources(
    _reference: &str,
    _directory_id: Option<&str>,
    _max_items: usize,
) -> Result<Vec<DirectoryResource>> {
    Err(Error::msg("directory queries are only available in the Wasm Host").into())
}

#[cfg(target_arch = "wasm32")]
fn read_all_reference(reference: &str, max_size: u64, chunk_size: u64) -> Result<Vec<u8>> {
    abi::content::guest::read_all(reference, max_size, chunk_size)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_all_reference(_reference: &str, _max_size: u64, _chunk_size: u64) -> Result<Vec<u8>> {
    Err(Error::msg("content references are only available in the Wasm Host").into())
}

pub enum View {
    Text(String),
    Markdown(String),
    Html { title: Option<String>, html: String },
    Frame(Frame),
    Json(Value),
    Media(Media),
    Download(Download),
}

impl View {
    pub fn json(value: impl Serialize) -> Result<Self> {
        Ok(Self::Json(serde_json::to_value(value)?))
    }
}

impl From<View> for PluginView {
    fn from(view: View) -> Self {
        match view {
            View::Text(text) => Self::Text(TextView { text }),
            View::Markdown(markdown) => Self::Markdown(MarkdownView { markdown }),
            View::Html { title, html } => Self::Html(HtmlView { title, html }),
            View::Frame(frame) => Self::PluginFrame(PluginFrameView {
                plugin_api: PLUGIN_API_VERSION.to_string(),
                title: frame.title,
                url: frame.url,
            }),
            View::Json(data) => Self::Json(JsonView { data }),
            View::Media(media) => Self::Media(MediaView {
                mime_type: media.mime_type,
                title: media.title,
                encoding: media.encoding,
                data: media.data,
            }),
            View::Download(download) => Self::Download(DownloadView {
                url: download.url,
                mime_type: download.mime_type,
                filename: download.filename,
            }),
        }
    }
}

pub struct Frame {
    title: Option<String>,
    url: String,
}

impl Frame {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            title: None,
            url: url.into(),
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

pub struct Media {
    mime_type: String,
    title: Option<String>,
    encoding: PluginMediaEncoding,
    data: String,
}

impl Media {
    pub fn base64(mime_type: impl Into<String>, bytes: impl AsRef<[u8]>) -> Self {
        Self::base64_data(mime_type, encode_base64(bytes))
    }

    pub fn base64_data(mime_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            mime_type: mime_type.into(),
            title: None,
            encoding: PluginMediaEncoding::Base64,
            data: data.into(),
        }
    }

    pub fn url(mime_type: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            mime_type: mime_type.into(),
            title: None,
            encoding: PluginMediaEncoding::Url,
            data: url.into(),
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

pub struct Download {
    url: String,
    mime_type: Option<String>,
    filename: Option<String>,
}

impl Download {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            mime_type: None,
            filename: None,
        }
    }

    pub fn mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    pub fn filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }
}

pub struct ResourceResponse {
    inner: PluginResourceActionOutput,
}

impl ResourceResponse {
    pub fn view(view: View) -> Self {
        Self {
            inner: PluginResourceActionOutput::new(view.into()),
        }
    }

    pub fn json(value: impl Serialize) -> Result<Self> {
        Ok(Self::view(View::json(value)?))
    }

    pub fn frame(frame: Frame) -> Self {
        Self::view(View::Frame(frame))
    }

    pub fn media(media: Media) -> Self {
        Self::view(View::Media(media))
    }

    pub fn without_view() -> Self {
        Self {
            inner: PluginResourceActionOutput::without_view(),
        }
    }

    pub fn replace_content(mut self, bytes: impl AsRef<[u8]>, mime_type: Option<&str>) -> Self {
        self.inner
            .effects
            .push(PluginResourceActionEffect::ReplaceContent(
                ReplaceContentEffect {
                    encoding: PluginReplacementEncoding::Base64,
                    data: encode_base64(bytes),
                    mime_type: mime_type.map(str::to_string),
                },
            ));
        self
    }

    pub fn delete(mut self) -> Self {
        self.inner.effects.push(PluginResourceActionEffect::Delete);
        self
    }
}

pub struct DirectoryResponse {
    inner: PluginDirectoryActionOutput,
}

impl DirectoryResponse {
    pub fn view(view: View) -> Self {
        Self {
            inner: PluginDirectoryActionOutput::new(view.into()),
        }
    }

    pub fn json(value: impl Serialize) -> Result<Self> {
        Ok(Self::view(View::json(value)?))
    }

    pub fn frame(frame: Frame) -> Self {
        Self::view(View::Frame(frame))
    }

    pub fn media(media: Media) -> Self {
        Self::view(View::Media(media))
    }

    pub fn without_view() -> Self {
        Self {
            inner: PluginDirectoryActionOutput::without_view(),
        }
    }

    pub fn create_tree(mut self, tree: Tree) -> Self {
        self.inner
            .effects
            .push(DirectoryActionEffect::CreateTree(tree.inner));
        self
    }

    pub fn update(
        mut self,
        name: Option<&str>,
        parent_id: Option<&str>,
        kind: Option<&str>,
    ) -> Self {
        self.inner
            .effects
            .push(DirectoryActionEffect::Update(UpdateDirectoryEffect {
                name: name.map(str::to_string),
                parent_id: parent_id.map(str::to_string),
                kind: kind.map(str::to_string),
            }));
        self
    }

    pub fn create_child(mut self, name: impl Into<String>, kind: Option<&str>) -> Self {
        self.inner.effects.push(DirectoryActionEffect::CreateChild(
            CreateChildDirectoryEffect {
                name: name.into(),
                kind: kind.map(str::to_string),
            },
        ));
        self
    }

    pub fn delete(mut self) -> Self {
        self.inner.effects.push(DirectoryActionEffect::Delete);
        self
    }
}

/// Builder for the bounded `create_tree` effect.
pub struct Tree {
    inner: CreateDirectoryTreeEffect,
}

impl Default for Tree {
    fn default() -> Self {
        Self {
            inner: CreateDirectoryTreeEffect {
                directories: Vec::new(),
                resources: Vec::new(),
            },
        }
    }
}

impl Tree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn directory(mut self, path: impl Into<String>) -> Self {
        self.inner.directories.push(CreateTreeDirectory {
            path: path.into(),
            kind: None,
        });
        self
    }

    pub fn directory_kind(mut self, path: impl Into<String>, kind: impl Into<String>) -> Self {
        self.inner.directories.push(CreateTreeDirectory {
            path: path.into(),
            kind: Some(kind.into()),
        });
        self
    }

    pub fn text(
        self,
        directory: impl Into<String>,
        name: impl Into<String>,
        text: impl AsRef<str>,
    ) -> Self {
        self.resource(
            directory,
            name,
            text.as_ref(),
            None,
            Some("text/plain; charset=utf-8"),
        )
    }

    pub fn markdown(
        self,
        directory: impl Into<String>,
        name: impl Into<String>,
        markdown: impl AsRef<str>,
    ) -> Self {
        self.resource(
            directory,
            name,
            markdown.as_ref(),
            None,
            Some("text/markdown; charset=utf-8"),
        )
    }

    pub fn resource(
        mut self,
        directory: impl Into<String>,
        name: impl Into<String>,
        bytes: impl AsRef<[u8]>,
        kind: Option<&str>,
        mime_type: Option<&str>,
    ) -> Self {
        self.inner.resources.push(CreateTreeResource {
            directory: directory.into(),
            name: name.into(),
            kind: kind.map(str::to_string),
            mime_type: mime_type.map(str::to_string),
            encoding: CreateTreeResourceEncoding::Base64,
            data: encode_base64(bytes),
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{Frame, ResourceResponse, run_resource_action};

    #[test]
    fn authoring_facade_owns_frame_version_and_structured_failures() {
        let input = serde_json::json!({
            "action": "example.open",
            "access": "read",
            "resource": {
                "id": "01900000-0000-7000-8000-000000000000",
                "directory": "docs",
                "name": "demo.txt",
                "kind": "core:resource",
                "revision": 1,
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z"
            }
        })
        .to_string();

        let output = run_resource_action(input.clone(), |_| {
            Ok(ResourceResponse::frame(Frame::new("index.html")))
        })
        .unwrap();
        let output: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(output["plugin_api"], crate::protocol::PLUGIN_API_VERSION);

        let failure = run_resource_action(input, |_| {
            Err(extism_pdk::Error::msg("broken action").into())
        })
        .unwrap();
        let failure: serde_json::Value = serde_json::from_str(&failure).unwrap();
        assert_eq!(failure["error"]["code"], "plugin.action_failed");
    }
}
