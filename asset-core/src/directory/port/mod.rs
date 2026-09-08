//! 目录聚合、查询与类型运行时端口。

mod persistence;

pub use persistence::{
    DirectoryIndex, DirectoryProjection, DirectoryQuery, DirectoryRelocation,
    DirectoryRelocationStore, DirectoryRevisionUpdate, DirectoryStore,
};
