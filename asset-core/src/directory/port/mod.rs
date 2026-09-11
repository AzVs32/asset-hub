//! 目录聚合持久化、读取模型与恢复端口。

mod persistence;

pub use persistence::{
    DirectoryIndex, DirectoryProjection, DirectoryReadModel, DirectoryRelocation,
    DirectoryRelocationStore, DirectoryRevisionUpdate, DirectoryStore,
};
