use crate::mount::Mount;
use crate::namespace::{VirtualPath, VirtualRelativePath};
use getset::{CopyGetters, Getters};

/// A mount selected for a request together with the request's path relative to it.
///
/// This is a derived value: it can only be created when the mount covers the
/// requested virtual path.
#[derive(Debug, PartialEq, Eq, CopyGetters, Getters)]
pub struct ResolvedMount<'a> {
    /// Returns the selected mount.
    #[getset(get_copy = "pub")]
    mount: &'a Mount,
    /// Returns the request path relative to the mount point.
    #[getset(get = "pub")]
    relative_path: VirtualRelativePath,
}

impl<'a> ResolvedMount<'a> {
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "The mount service has not been migrated yet.")
    )]
    pub(crate) fn new(mount: &'a Mount, path: &VirtualPath) -> Option<Self> {
        let relative_path = path.strip_prefix(mount.virtual_path())?;

        Some(Self {
            mount,
            relative_path,
        })
    }
}

#[cfg(test)]
mod tests;
