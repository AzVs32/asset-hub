//! Narrow application coordinator for cross-aggregate projections.

use super::{DirectoryService, ResourceService};
use crate::CoreError;
use crate::domain::{Checksum, DirectoryId, ResourceId};
use crate::port::ListResources;
use std::collections::VecDeque;

const DIRECTORY_ARCHIVE_PAGE_SIZE: u32 = 100;

/// Coordinates only operations whose consistency boundary spans Resource and Directory.
#[derive(Clone)]
pub struct AssetWorkflowService {
    resources: ResourceService,
    directories: DirectoryService,
}

impl AssetWorkflowService {
    pub fn new(resources: ResourceService, directories: DirectoryService) -> Self {
        Self {
            resources,
            directories,
        }
    }

    /// Build a best-effort manifest for a directory tree addressed by its global stable ID.
    ///
    /// The manifest retains canonical archive-path validation and contains only Resources with
    /// content. Enumeration is not transactional; paths and content expectations are captured
    /// per entry, not at a single point in time.
    pub async fn directory_archive_manifest(
        &self,
        id: &DirectoryId,
    ) -> Result<DirectoryArchiveManifest, CoreError> {
        let root = self.directories.find_by_id(id).await?;
        let archive_root = if root.id().is_root() {
            "asset-hub".to_string()
        } else {
            root.directory().name().to_string()
        };
        let filename = format!("{archive_root}.zip");
        let root_path = root.path().path().to_string();
        let mut pending = VecDeque::from([root]);
        let mut directories = Vec::new();
        let mut resources = Vec::new();

        while let Some(directory) = pending.pop_front() {
            if !self.directories.contains(id, &directory.id()).await? {
                continue;
            }
            let archive_path =
                directory_archive_path(&archive_root, &root_path, directory.path().path())?;
            directories.push(format!("{archive_path}/"));

            let mut offset = 0;
            loop {
                let page = self
                    .resources
                    .list(ListResources::new(
                        DIRECTORY_ARCHIVE_PAGE_SIZE,
                        offset,
                        directory.id(),
                    ))
                    .await?;
                let item_count = page.items.len() as u64;
                resources.extend(page.items.into_iter().filter_map(|located| {
                    let resource = located.resource();
                    resource.content().map(|content| {
                        DirectoryArchiveResource::new(
                            resource.id(),
                            format!("{archive_path}/{}", resource.name()),
                            content.size(),
                            content.checksum().cloned(),
                        )
                    })
                }));
                offset += item_count;
                if offset >= page.total || item_count == 0 {
                    break;
                }
            }
            pending.extend(self.directories.list_children(&directory.id()).await?);
        }

        directories.sort();
        resources.sort_by(|left, right| left.path().cmp(right.path()));
        Ok(DirectoryArchiveManifest::new(
            filename,
            directories,
            resources,
        ))
    }
}

/// Best-effort directory projection with per-resource content expectations for ZIP generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryArchiveManifest {
    filename: String,
    directories: Vec<String>,
    resources: Vec<DirectoryArchiveResource>,
}

impl DirectoryArchiveManifest {
    fn new(
        filename: String,
        directories: Vec<String>,
        resources: Vec<DirectoryArchiveResource>,
    ) -> Self {
        Self {
            filename,
            directories,
            resources,
        }
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn directories(&self) -> &[String] {
        &self.directories
    }

    pub fn resources(&self) -> &[DirectoryArchiveResource] {
        &self.resources
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryArchiveResource {
    resource_id: ResourceId,
    path: String,
    content_length: u64,
    checksum: Option<Checksum>,
}

impl DirectoryArchiveResource {
    fn new(
        resource_id: ResourceId,
        path: String,
        content_length: u64,
        checksum: Option<Checksum>,
    ) -> Self {
        Self {
            resource_id,
            path,
            content_length,
            checksum,
        }
    }

    pub fn resource_id(&self) -> ResourceId {
        self.resource_id
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn checksum(&self) -> Option<&Checksum> {
        self.checksum.as_ref()
    }

    pub fn content_length(&self) -> u64 {
        self.content_length
    }
}

fn directory_archive_path(
    archive_root: &str,
    root_path: &str,
    directory_path: &str,
) -> Result<String, CoreError> {
    if directory_path == root_path {
        return Ok(archive_root.to_string());
    }
    let relative = if root_path.is_empty() {
        directory_path
    } else {
        directory_path
            .strip_prefix(root_path)
            .and_then(|path| path.strip_prefix('/'))
            .ok_or_else(|| CoreError::invariant("directory is outside archive root"))?
    };
    if relative.is_empty() {
        return Ok(archive_root.to_string());
    }
    Ok(format!("{archive_root}/{relative}"))
}
