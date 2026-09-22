use crate::namespace::domain::{VirtualPath, VirtualRelativePath};

use super::Mount;

/// A mount selected for a request together with the request's path relative to it.
///
/// This is a derived value: it can only be created when the mount covers the
/// requested virtual path.
#[derive(Debug, PartialEq, Eq)]
pub struct ResolvedMount<'a> {
    mount: &'a Mount,
    relative_path: VirtualRelativePath,
}

impl<'a> ResolvedMount<'a> {
    pub(crate) fn new(mount: &'a Mount, path: &VirtualPath) -> Option<Self> {
        let relative_path = path.strip_prefix(mount.virtual_path())?;

        Some(Self {
            mount,
            relative_path,
        })
    }

    /// Returns the selected mount.
    pub fn mount(&self) -> &'a Mount {
        self.mount
    }

    /// Returns the request path relative to the mount point.
    pub fn relative_path(&self) -> &VirtualRelativePath {
        &self.relative_path
    }
}
