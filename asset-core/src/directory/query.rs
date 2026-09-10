//! 应用服务和读取模型适配器共享的目录查询结果。

use crate::directory::domain::{Directory, DirectoryId, DirectoryPath};

/// 一个完整的目录聚合及其当前的路径投影。
#[derive(Debug, Clone, PartialEq)]
pub struct LocatedDirectory {
    directory: Directory,
    path: DirectoryPath,
}

impl LocatedDirectory {
    pub fn new(directory: Directory, path: DirectoryPath) -> Self {
        Self { directory, path }
    }

    pub fn directory(&self) -> &Directory {
        &self.directory
    }

    pub fn id(&self) -> DirectoryId {
        self.directory.id()
    }

    pub fn path(&self) -> &DirectoryPath {
        &self.path
    }

    pub fn into_directory(self) -> Directory {
        self.directory
    }

    pub fn into_parts(self) -> (Directory, DirectoryPath) {
        (self.directory, self.path)
    }
}
