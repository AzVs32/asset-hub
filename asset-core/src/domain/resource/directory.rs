use crate::UserError;
use crate::domain::normalize_directory;

/// 可独立存在的逻辑资源目录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDirectory {
    path: String,
    parent_path: String,
    name: String,
}

impl ResourceDirectory {
    pub fn new(parent_path: impl Into<String>, name: impl Into<String>) -> Result<Self, UserError> {
        let parent_path = normalize_directory(parent_path.into())?;
        let name = name.into();
        let name = name.trim();
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains('/')
            || name.contains('\\')
        {
            return Err(UserError::InvalidDirectory);
        }
        let path = if parent_path.is_empty() {
            name.to_owned()
        } else {
            format!("{parent_path}/{name}")
        };
        normalize_directory(path.clone())?;
        Ok(Self {
            path,
            parent_path,
            name: name.to_owned(),
        })
    }
    pub fn rehydrate(path: String, parent_path: String, name: String) -> Result<Self, UserError> {
        let directory = Self::new(parent_path, name)?;
        if directory.path != normalize_directory(path)? {
            return Err(UserError::InvalidDirectory);
        }
        Ok(directory)
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn parent_path(&self) -> &str {
        &self.parent_path
    }
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_is_built_from_parent_and_single_name() {
        let directory = ResourceDirectory::new("projects", " images ").unwrap();
        assert_eq!(directory.path(), "projects/images");
        assert!(ResourceDirectory::new("projects", "../secret").is_err());
    }
}
