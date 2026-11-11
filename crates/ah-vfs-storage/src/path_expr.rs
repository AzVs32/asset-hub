//! Path operands, before any directory or symbolic link is looked up.
use super::{EntryName, MAX_PATH_BYTES, NameError, Rejection, StoragePath};
use std::{fmt, str::FromStr};
use thiserror::Error;

/// Raw operand bound, including an optional leading and trailing slash in
/// addition to the maximum length of a selected instance-relative address.
pub const MAX_PATH_EXPR_BYTES: usize = MAX_PATH_BYTES + 2;

/// All starting points in the path grammar. Executors can match exhaustively.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LookupStart {
    InstanceRoot,
    /// The directory selected from the current request's base address, or the
    /// containing directory of a link when interpreting its target.
    BaseDirectory,
}

/// All component forms in the path grammar. Executors can match exhaustively.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum PathComponent {
    Name(EntryName),
    CurrentDirectory,
    ParentDirectory,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum PathExprError {
    #[error("path operand is empty")]
    Empty,
    #[error("path operand exceeds the UTF-8 byte limit")]
    TooLong,
    #[error("path operand contains an invalid name")]
    InvalidName(#[from] NameError),
}

/// A nonempty, instance-scoped path operand. A leading slash selects the
/// instance root. A relative operand uses the directory selected from the base
/// address. A relative link target uses the link's containing directory.
/// Runs of slashes are equivalent to one slash.
/// Ordinary components retain their exact [`EntryName`] spelling.
/// Use [`StoragePath::to_rooted_expr`] to reuse an already selected address.
///
/// Dot components are retained for sequential lookup. In `a/../b`, `a` must
/// first be looked up as a directory, including any permitted link traversal.
/// Parent traversal from the instance root stays at that root. A trailing
/// slash requires a directory, including when the operation creates it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PathExpr {
    start: LookupStart,
    components: Vec<PathComponent>,
    trailing_slash: bool,
}

impl PathExpr {
    pub fn parse(input: &str) -> Result<Self, PathExprError> {
        if input.is_empty() {
            return Err(PathExprError::Empty);
        }
        if input.len() > MAX_PATH_EXPR_BYTES {
            return Err(PathExprError::TooLong);
        }

        let start = if input.starts_with('/') {
            LookupStart::InstanceRoot
        } else {
            LookupStart::BaseDirectory
        };
        let mut components = Vec::new();
        for component in input.split('/').filter(|component| !component.is_empty()) {
            components.push(match component {
                "." => PathComponent::CurrentDirectory,
                ".." => PathComponent::ParentDirectory,
                name => PathComponent::Name(EntryName::parse(name)?),
            });
        }

        Ok(Self {
            start,
            components,
            trailing_slash: input.ends_with('/'),
        })
    }

    pub fn start(&self) -> LookupStart {
        self.start
    }

    pub fn components(&self) -> &[PathComponent] {
        &self.components
    }

    /// Retain an explicit directory requirement when routing an operand to an instance.
    pub fn with_trailing_slash(mut self) -> Self {
        self.trailing_slash = true;
        self
    }

    pub fn has_trailing_slash(&self) -> bool {
        self.trailing_slash
    }

    /// The operand denotes a directory if it ends in a slash, `.` or `..`,
    /// including the root operand `/`.
    pub fn requires_directory(&self) -> bool {
        self.trailing_slash
            || matches!(
                self.components.last(),
                Some(PathComponent::CurrentDirectory | PathComponent::ParentDirectory)
            )
    }

    /// Check the end of this segment after traversal, including when it is a
    /// followed link target and the original operand has no trailing slash.
    pub fn check_directory_requirement(&self, is_directory: bool) -> Result<(), Rejection> {
        if self.requires_directory() && !is_directory {
            return Err(Rejection::NotDirectory);
        }
        Ok(())
    }

    /// Split a final ordinary name from its unevaluated parent operand.
    /// The caller still applies the original operand's directory requirement.
    pub fn split_final_name(&self) -> Option<(Self, &EntryName)> {
        let (PathComponent::Name(name), parents) = self.components.split_last()? else {
            return None;
        };
        let components = if parents.is_empty() && self.start == LookupStart::BaseDirectory {
            vec![PathComponent::CurrentDirectory]
        } else {
            parents.to_vec()
        };
        Some((
            Self {
                start: self.start,
                components,
                trailing_slash: parents.is_empty() && self.start == LookupStart::InstanceRoot,
            },
            name,
        ))
    }

    pub(super) fn ends_in_name(&self) -> bool {
        matches!(self.components.last(), Some(PathComponent::Name(_)))
    }
}

impl StoragePath {
    /// Reuse this address as a path operand anchored at the instance root.
    /// The root becomes `/`. All other addresses retain their exact components
    /// without a trailing slash. Lookup applies the new operation's normal
    /// rules to this address, using the current state of the instance.
    pub fn to_rooted_expr(&self) -> PathExpr {
        PathExpr {
            start: LookupStart::InstanceRoot,
            components: self.entry_names().map(PathComponent::Name).collect(),
            trailing_slash: self.is_root(),
        }
    }
}

impl FromStr for PathExpr {
    type Err = PathExprError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl fmt::Display for PathExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.start == LookupStart::InstanceRoot {
            f.write_str("/")?;
        }
        for (index, component) in self.components.iter().enumerate() {
            if index > 0 {
                f.write_str("/")?;
            }
            match component {
                PathComponent::Name(name) => f.write_str(name.as_str())?,
                PathComponent::CurrentDirectory => f.write_str(".")?,
                PathComponent::ParentDirectory => f.write_str("..")?,
            }
        }
        if self.trailing_slash && !self.components.is_empty() {
            f.write_str("/")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_operands_preserve_lookup_order_and_directory_requirement() -> Result<(), PathExprError>
    {
        let absolute = PathExpr::parse("//docs//a")?;
        let relative = PathExpr::parse("docs/a")?;
        assert_eq!(absolute.start(), LookupStart::InstanceRoot);
        assert_eq!(relative.start(), LookupStart::BaseDirectory);
        assert_eq!(absolute.components(), relative.components());
        assert_eq!(absolute.to_string(), "/docs/a");

        let path = PathExpr::parse("missing//.././a/")?;
        assert!(matches!(
            path.components(),
            [PathComponent::Name(first), PathComponent::ParentDirectory,
             PathComponent::CurrentDirectory, PathComponent::Name(last)]
                if first.as_str() == "missing" && last.as_str() == "a"
        ));
        assert!(path.has_trailing_slash());
        assert!(path.requires_directory());
        assert_eq!(PathExpr::parse(&path.to_string())?, path);

        let root = PathExpr::parse("///")?;
        assert_eq!(root.start(), LookupStart::InstanceRoot);
        assert!(root.components().is_empty());
        assert!(root.requires_directory());
        assert_eq!(root.to_string(), "/");
        assert_eq!(
            PathExpr::parse("/..")?.components(),
            &[PathComponent::ParentDirectory]
        );
        for input in [".", "..", "a/.", "a/.."] {
            assert!(PathExpr::parse(input)?.requires_directory());
        }
        assert!(!relative.requires_directory());
        Ok(())
    }

    #[test]
    fn operands_check_empty_input_raw_bytes_and_ordinary_names() {
        assert_eq!(PathExpr::parse(""), Err(PathExprError::Empty));
        assert_eq!(
            PathExpr::parse(&"/".repeat(MAX_PATH_EXPR_BYTES + 1)),
            Err(PathExprError::TooLong)
        );
        assert_eq!(
            PathExpr::parse("dir/a\0b"),
            Err(PathExprError::InvalidName(NameError::ForbiddenCharacter))
        );
    }

    #[test]
    fn full_length_address_accepts_both_anchor_and_directory_slash() -> Result<(), PathExprError> {
        let name = "a".repeat(MAX_PATH_BYTES);
        let path = PathExpr::parse(&format!("/{name}/"))?;
        assert_eq!(path.start(), LookupStart::InstanceRoot);
        assert!(path.requires_directory());
        Ok(())
    }
}
