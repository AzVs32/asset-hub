use crate::error::VfsError;
use crate::namespace::error::NamespaceError;

use crate::namespace::{EntryName, VirtualRelativePath};

const MAX_BYTES: usize = 4096;

/// A canonical, absolute path in the platform-independent virtual namespace.
///
/// Paths are case-sensitive, preserve spaces exactly, and measure limits in
/// UTF-8 bytes. Unicode is preserved without normalization. `/` is the only
/// separator; trailing separators, empty segments, dot segments, backslashes,
/// and control characters are rejected.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VirtualPath(String);

impl VirtualPath {
    /// Returns the root virtual path.
    pub fn root() -> Self {
        Self("/".to_owned())
    }

    /// Returns the canonical string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns whether this path is the virtual root.
    pub fn is_root(&self) -> bool {
        self.0 == "/"
    }

    /// Returns the number of segments in this path.
    pub fn depth(&self) -> usize {
        if self.is_root() {
            0
        } else {
            self.0[1..].split('/').count()
        }
    }

    /// Returns the parent path, or `None` for the virtual root.
    pub fn parent(&self) -> Option<Self> {
        if self.is_root() {
            return None;
        }

        let separator = self.0.rfind('/').expect("a VirtualPath is always absolute");
        if separator == 0 {
            Some(Self::root())
        } else {
            Some(Self(self.0[..separator].to_owned()))
        }
    }

    /// Returns the final path segment, or `None` for the virtual root.
    pub fn name(&self) -> Option<&str> {
        if self.is_root() {
            None
        } else {
            self.0.rsplit('/').next()
        }
    }

    /// Validates and appends one raw segment to this path.
    pub fn join_segment(&self, segment: impl AsRef<str>) -> Result<Self, VfsError> {
        let name = EntryName::try_from(segment.as_ref())?;
        self.join_name(&name)
    }

    /// Appends an already validated entry name to this path.
    pub fn join_name(&self, name: &EntryName) -> Result<Self, VfsError> {
        let name = name.as_str();

        let length = self.0.len() + usize::from(!self.is_root()) + name.len();
        if length > MAX_BYTES {
            return Err(NamespaceError::VirtualPathTooLong {
                length,
                max: MAX_BYTES,
            }
            .into());
        }

        let joined = if self.is_root() {
            format!("/{name}")
        } else {
            format!("{}/{name}", self.0)
        };

        Ok(Self(joined))
    }

    /// Removes an ancestor prefix and returns the remaining relative path.
    pub fn strip_prefix(&self, base: &Self) -> Option<VirtualRelativePath> {
        let relative_path = if self == base {
            ""
        } else if base.is_root() {
            self.0.strip_prefix('/')?
        } else {
            self.0.strip_prefix(base.as_str())?.strip_prefix('/')?
        };

        Some(VirtualRelativePath::from_validated(relative_path))
    }

    /// Returns whether this path is a strict ancestor of the other path.
    pub fn is_ancestor_of(&self, other: &Self) -> bool {
        self != other && self.is_ancestor_or_self_of(other)
    }

    /// Returns whether this path is an ancestor of or equal to the other path.
    pub fn is_ancestor_or_self_of(&self, other: &Self) -> bool {
        if self.is_root() || self == other {
            return true;
        }

        other
            .as_str()
            .strip_prefix(self.as_str())
            .is_some_and(|remaining| remaining.starts_with('/'))
    }

    fn validate(path: &str) -> Result<(), NamespaceError> {
        if path.len() > MAX_BYTES {
            return Err(NamespaceError::VirtualPathTooLong {
                length: path.len(),
                max: MAX_BYTES,
            });
        }

        if !path.starts_with('/') || path.contains("//") || (path != "/" && path.ends_with('/')) {
            return Err(NamespaceError::InvalidVirtualPath);
        }

        if path == "/" {
            return Ok(());
        }

        for segment in path[1..].split('/') {
            EntryName::validate(segment)?;
        }

        Ok(())
    }
}

impl std::fmt::Display for VirtualPath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for VirtualPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for VirtualPath {
    type Error = VfsError;

    fn try_from(path: &str) -> Result<Self, Self::Error> {
        Self::validate(path)?;
        Ok(Self(path.to_owned()))
    }
}

#[cfg(test)]
mod tests;
