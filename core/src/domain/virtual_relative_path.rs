/// A canonical path relative to a point in the virtual namespace.
///
/// A relative virtual path is derived from validated [`crate::domain::VirtualPath`] values,
/// so it inherits their segment, case, Unicode, and length rules. The empty
/// path represents the relative root.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VirtualRelativePath(String);

impl VirtualRelativePath {
    pub(super) fn from_validated(path: &str) -> Self {
        debug_assert!(!path.starts_with('/'));
        debug_assert!(path.is_empty() || !path.ends_with('/'));

        Self(path.to_owned())
    }

    /// Returns the canonical relative path representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns whether this path represents the relative root.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the number of segments in this relative path.
    pub fn depth(&self) -> usize {
        if self.is_empty() {
            0
        } else {
            self.0.split('/').count()
        }
    }

    /// Returns the final path segment, or `None` for the relative root.
    pub fn name(&self) -> Option<&str> {
        if self.is_empty() {
            None
        } else {
            self.0.rsplit('/').next()
        }
    }
}

impl std::fmt::Display for VirtualRelativePath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for VirtualRelativePath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
