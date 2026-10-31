use derive_more::Display;
use std::str::FromStr;
use thiserror::Error;

/// An absolute, canonical path in the virtual filesystem, not an OS path.
///
/// Paths use `/` on every platform. Repeated and trailing separators are removed;
/// dot segments, backslashes and NUL are rejected instead of reinterpreted.
/// Unicode names are preserved exactly, without case folding or normalization.
#[derive(Clone, Debug, Display, Eq, PartialEq)]
pub struct VirtualPath(String);

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum VirtualPathError {
    #[error("virtual paths must start with '/'")]
    NotAbsolute,
    #[error("virtual path exceeds the UTF-8 byte limit")]
    TooLong,
    #[error("virtual paths cannot contain backslashes or NUL")]
    InvalidCharacter,
    #[error("virtual paths cannot contain '.' or '..' segments")]
    DotSegment,
    #[error("a child name must be one non-empty path segment")]
    InvalidChildName,
}

impl VirtualPath {
    /// Maximum input size in UTF-8 bytes, independent of native filesystem limits.
    pub const MAX_LEN: usize = 4096;

    pub fn parse(input: &str) -> Result<Self, VirtualPathError> {
        if !input.starts_with('/') {
            return Err(VirtualPathError::NotAbsolute);
        }
        if input.len() > Self::MAX_LEN {
            return Err(VirtualPathError::TooLong);
        }
        if input.contains(['\\', '\0']) {
            return Err(VirtualPathError::InvalidCharacter);
        }

        let mut canonical = String::with_capacity(input.len());
        for segment in input.split('/').filter(|segment| !segment.is_empty()) {
            if matches!(segment, "." | "..") {
                return Err(VirtualPathError::DotSegment);
            }
            canonical.push('/');
            canonical.push_str(segment);
        }
        if canonical.is_empty() {
            canonical.push('/');
        }
        Ok(Self(canonical))
    }

    pub fn root() -> Self {
        Self("/".into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.0 == "/"
    }

    /// Borrow the final path segment, or return `None` for the root.
    pub fn name(&self) -> Option<&str> {
        self.segments().next_back()
    }

    /// Iterate over non-empty path segments, excluding `/` separators.
    ///
    /// Segments are borrowed in path order. The root has no segments.
    pub fn segments(&self) -> impl DoubleEndedIterator<Item = &str> + '_ {
        self.0.split('/').filter(|segment| !segment.is_empty())
    }

    /// Remove `base` at a whole-segment boundary and borrow the relative suffix.
    ///
    /// Returns `None` if `base` is neither this path nor an ancestor. The suffix
    /// has no leading `/`; equal paths yield `Some("")`, and a root base removes
    /// only the leading `/`.
    pub fn strip_prefix(&self, base: &Self) -> Option<&str> {
        let suffix = self.as_str().strip_prefix(base.as_str())?;
        if base.is_root() || suffix.is_empty() {
            Some(suffix)
        } else {
            suffix.strip_prefix('/')
        }
    }

    /// Return whether `other` is this path or one of its descendants.
    ///
    /// Matching respects whole path segments and is case-sensitive.
    /// The root contains every virtual path, including itself.
    pub fn contains(&self, other: &Self) -> bool {
        other.strip_prefix(self).is_some()
    }

    /// Return the immediate parent, or `None` for the root.
    pub fn parent(&self) -> Option<Self> {
        if self.is_root() {
            return None;
        }
        let (parent, _) = self.0.rsplit_once('/')?;
        Some(if parent.is_empty() {
            Self::root()
        } else {
            Self(parent.into())
        })
    }

    /// Return a new path with exactly one child name appended.
    ///
    /// `name` must be a single non-empty segment. Names are preserved exactly.
    ///
    /// # Errors
    ///
    /// Returns [`VirtualPathError::InvalidChildName`] for an empty name or one
    /// containing `/`, [`VirtualPathError::InvalidCharacter`] for `\` or NUL,
    /// [`VirtualPathError::DotSegment`] for `.` or `..`, or
    /// [`VirtualPathError::TooLong`] if the full path exceeds [`Self::MAX_LEN`]
    /// UTF-8 bytes.
    pub fn join(&self, name: &str) -> Result<Self, VirtualPathError> {
        if name.is_empty() || name.contains('/') {
            return Err(VirtualPathError::InvalidChildName);
        }
        let separator = if self.is_root() { "" } else { "/" };
        Self::parse(&format!("{}{separator}{name}", self.as_str()))
    }
}

impl FromStr for VirtualPath {
    type Err = VirtualPathError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl TryFrom<String> for VirtualPath {
    type Error = VirtualPathError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equivalent_spellings_have_one_canonical_identity() -> Result<(), VirtualPathError> {
        assert_eq!(VirtualPath::parse("////")?, VirtualPath::root());
        assert_eq!(
            VirtualPath::parse("//文档///保存/")?,
            VirtualPath::parse("/文档/保存")?
        );
        assert_eq!(VirtualPath::parse("/文档/")?.as_str(), "/文档");
        Ok(())
    }

    #[test]
    fn rejects_ambiguous_or_native_paths() {
        for input in [
            "", "relative", "C:\\data", "/a\\b", "/a\0b", "/./a", "/a/../b",
        ] {
            assert!(VirtualPath::parse(input).is_err(), "accepted {input:?}");
        }
    }

    #[test]
    fn containment_respects_segment_boundaries() -> Result<(), VirtualPathError> {
        let parent = VirtualPath::parse("/games")?;
        assert!(parent.contains(&parent));
        assert!(parent.contains(&VirtualPath::parse("/games/save")?));
        assert!(!parent.contains(&VirtualPath::parse("/games-old")?));
        assert!(!parent.contains(&VirtualPath::root()));
        assert!(VirtualPath::root().contains(&parent));
        Ok(())
    }

    #[test]
    fn segments_reconstruct_canonical_paths_and_expose_the_final_name()
    -> Result<(), VirtualPathError> {
        for input in [
            "/",
            "////",
            "/docs",
            "//文档///保存/",
            "/a/.hidden/ spaced /😀",
        ] {
            let path = VirtualPath::parse(input)?;
            let segments: Vec<_> = path.segments().collect();
            assert!(segments.iter().all(|segment| !segment.is_empty()));
            assert_eq!(path.name(), segments.last().copied());
            assert_eq!(
                path.segments().rev().collect::<Vec<_>>(),
                segments.iter().copied().rev().collect::<Vec<_>>()
            );

            let rebuilt = segments
                .iter()
                .try_fold(VirtualPath::root(), |parent, name| parent.join(name))?;
            assert_eq!(rebuilt, path);
        }
        assert_eq!(VirtualPath::parse("//文档///保存/")?.name(), Some("保存"));
        assert_eq!(VirtualPath::root().name(), None);
        Ok(())
    }

    #[test]
    fn prefix_stripping_respects_roots_boundaries_and_unicode() -> Result<(), VirtualPathError> {
        for (input, base, expected) in [
            ("/", "/", Some("")),
            ("/docs/a.txt", "/", Some("docs/a.txt")),
            ("/docs", "/docs", Some("")),
            ("/docs/reports/a.txt", "/docs", Some("reports/a.txt")),
            ("//文档///保存/😀", "/文档/", Some("保存/😀")),
            ("/docs-old/a.txt", "/docs", None),
            ("/文档库/保存", "/文档", None),
            ("/docs", "/docs/a.txt", None),
            ("/", "/docs", None),
            ("/Docs/a.txt", "/docs", None),
            ("/é/a", "/e\u{301}", None),
            ("/other/a.txt", "/docs", None),
        ] {
            let path = VirtualPath::parse(input)?;
            let base = VirtualPath::parse(base)?;
            let suffix = path.strip_prefix(&base);
            assert_eq!(suffix, expected, "path={input:?}, base={base}");
            assert_eq!(base.contains(&path), suffix.is_some());
            if let Some(suffix) = suffix {
                let rebuilt = suffix
                    .split('/')
                    .filter(|name| !name.is_empty())
                    .try_fold(base, |parent, name| parent.join(name))?;
                assert_eq!(rebuilt, path);
            }
        }
        Ok(())
    }

    #[test]
    fn stripped_suffix_can_outlive_the_base() -> Result<(), VirtualPathError> {
        let path = VirtualPath::parse("/docs/report.txt")?;
        let suffix = {
            let base = VirtualPath::parse("/docs")?;
            path.strip_prefix(&base)
        };
        assert_eq!(suffix, Some("report.txt"));
        Ok(())
    }

    #[test]
    fn root_and_child_navigation_preserve_path_invariants() -> Result<(), VirtualPathError> {
        let root = VirtualPath::root();
        let child = root.join("文档")?;
        assert_eq!(root.parent(), None);
        assert_eq!(child.parent(), Some(root));
        assert_eq!(child.join("保存")?.parent(), Some(child.clone()));
        for name in ["", ".", "..", "a/b", "/a", "a\\b", "a\0b"] {
            assert!(child.join(name).is_err(), "accepted {name:?}");
        }
        Ok(())
    }

    #[test]
    fn length_limit_counts_utf8_bytes() -> Result<(), VirtualPathError> {
        let at_limit = format!("/{}", "a".repeat(VirtualPath::MAX_LEN - 1));
        let path = VirtualPath::parse(&at_limit)?;
        assert_eq!(path.as_str().len(), VirtualPath::MAX_LEN);
        assert_eq!(path.join("b"), Err(VirtualPathError::TooLong));

        let unicode_at_limit = format!("/{}", "中".repeat(VirtualPath::MAX_LEN / 3));
        assert_eq!(unicode_at_limit.len(), VirtualPath::MAX_LEN);
        assert!(VirtualPath::parse(&unicode_at_limit).is_ok());
        assert_eq!(
            VirtualPath::parse(&format!("{unicode_at_limit}a")),
            Err(VirtualPathError::TooLong)
        );
        Ok(())
    }
}
