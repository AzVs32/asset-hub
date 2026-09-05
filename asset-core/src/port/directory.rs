//! 目录聚合、查询与类型运行时端口。

mod kind;
mod persistence;

pub use kind::DirectoryKindRegistry;
pub use persistence::{
    DirectoryIndex, DirectoryLocation, DirectoryProjection, DirectoryQuery, DirectoryRelocation,
    DirectoryRelocationStore, DirectoryRevisionUpdate, DirectoryStore, LocatedDirectory,
};
