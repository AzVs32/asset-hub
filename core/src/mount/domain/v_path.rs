use crate::mount::error::DomainError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VPath(String);

impl VPath {
    /// Parses and normalizes an absolute virtual path.
    pub fn parse(path: impl AsRef<str>) -> Result<Self, DomainError> {
        let path = path.as_ref();

        // 接受两种路径分隔符，但必须以其中一种开头以保证路径是绝对路径。
        if !matches!(path.as_bytes().first(), Some(b'/' | b'\\')) {
            return Err(DomainError::VPathNotAbsolute);
        }

        // 同时按正斜杠和反斜杠拆分，为后续生成统一格式的路径段做准备。
        let mut segments = Vec::new();
        for segment in path.split(['/', '\\']) {
            match segment {
                // 忽略空路径段以折叠重复分隔符，并移除表示当前目录的点路径段。
                "" | "." => {}
                // 禁止双点路径段，避免路径向虚拟父级跳转。
                ".." => return Err(DomainError::VPathContainsDotDotSegment),
                segment => segments.push(segment),
            }
        }

        // 使用正斜杠重新连接路径段；没有有效路径段时规范化为根路径。
        let normalized = if segments.is_empty() {
            "/".to_owned()
        } else {
            format!("/{}", segments.join("/"))
        };

        Ok(Self(normalized))
    }

    /// Returns the root virtual path.
    pub fn root() -> Self {
        Self("/".to_owned())
    }

    /// Returns the normalized string representation.
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
}
