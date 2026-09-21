use crate::path::error::DomainError;

use super::v_relative_path::VRelativePath;

const MAX_BYTES: usize = 4096;
const MAX_SEGMENT_BYTES: usize = 255;

/// A canonical, absolute path in the platform-independent virtual namespace.
///
/// Paths are case-sensitive, preserve spaces exactly, and measure limits in
/// UTF-8 bytes. Unicode is preserved without normalization. `/` is the only
/// separator; trailing separators, empty segments, dot segments, backslashes,
/// and control characters are rejected.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VPath(String);

impl VPath {
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

        let separator = self.0.rfind('/').expect("a VPath is always absolute");
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

    /// Appends one validated segment to this path.
    pub fn join_segment(&self, segment: impl AsRef<str>) -> Result<Self, DomainError> {
        let segment = segment.as_ref();
        Self::validate_segment(segment)?;

        let length = self.0.len() + usize::from(!self.is_root()) + segment.len();
        if length > MAX_BYTES {
            return Err(DomainError::VPathTooLong {
                length,
                max: MAX_BYTES,
            });
        }

        let joined = if self.is_root() {
            format!("/{segment}")
        } else {
            format!("{}/{segment}", self.0)
        };

        Ok(Self(joined))
    }

    /// Removes an ancestor prefix and returns the remaining relative path.
    pub fn strip_prefix(&self, base: &Self) -> Option<VRelativePath> {
        let relative_path = if self == base {
            ""
        } else if base.is_root() {
            self.0.strip_prefix('/')?
        } else {
            self.0.strip_prefix(base.as_str())?.strip_prefix('/')?
        };

        Some(VRelativePath::from_validated(relative_path))
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

    fn validate_segment(segment: &str) -> Result<(), DomainError> {
        if segment.is_empty() {
            return Err(DomainError::VPathSegmentEmpty);
        }

        if segment.contains('/') {
            return Err(DomainError::VPathSegmentContainsSeparator);
        }

        if segment.contains('\\') {
            return Err(DomainError::VPathContainsBackslash);
        }

        if segment.chars().any(char::is_control) {
            return Err(DomainError::VPathContainsControlCharacter);
        }

        match segment {
            "." => return Err(DomainError::VPathContainsDotSegment),
            ".." => return Err(DomainError::VPathContainsDotDotSegment),
            _ => {}
        }

        if segment.len() > MAX_SEGMENT_BYTES {
            return Err(DomainError::VPathSegmentTooLong {
                length: segment.len(),
                max: MAX_SEGMENT_BYTES,
            });
        }

        Ok(())
    }
}

impl std::fmt::Display for VPath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for VPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for VPath {
    type Error = DomainError;

    fn try_from(path: &str) -> Result<Self, Self::Error> {
        if path.len() > MAX_BYTES {
            return Err(DomainError::VPathTooLong {
                length: path.len(),
                max: MAX_BYTES,
            });
        }

        if path.starts_with('\\') {
            return Err(DomainError::VPathContainsBackslash);
        }

        if !path.starts_with('/') {
            return Err(DomainError::VPathNotAbsolute);
        }

        if path == "/" {
            return Ok(Self::root());
        }

        if path.contains("//") {
            return Err(DomainError::VPathContainsRepeatedSeparator);
        }

        if path.ends_with('/') {
            return Err(DomainError::VPathHasTrailingSeparator);
        }

        for segment in path[1..].split('/') {
            Self::validate_segment(segment)?;
        }

        Ok(Self(path.to_owned()))
    }
}
