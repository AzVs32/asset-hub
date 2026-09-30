use crate::mount::Mount;
use crate::namespace::{VirtualPath, VirtualRelativePath};
use getset::Getters;

/// A mount selected for a request together with the request's path relative to it.
///
/// This is a derived value: it can only be created when the mount covers the
/// requested virtual path.
#[derive(Debug, Clone, PartialEq, Eq, Getters)]
pub struct ResolvedMount {
    /// Returns the selected mount.
    #[getset(get = "pub")]
    mount: Mount,
    /// Returns the request path relative to the mount point.
    #[getset(get = "pub")]
    relative_path: VirtualRelativePath,
}

impl ResolvedMount {
    pub(crate) fn new(mount: Mount, path: &VirtualPath) -> Option<Self> {
        let relative_path = path.strip_prefix(mount.virtual_path())?;

        Some(Self {
            mount,
            relative_path,
        })
    }
}

#[cfg(test)]
mod tests;
