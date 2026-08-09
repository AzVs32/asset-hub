//! 目录聚合、查询、类型与动作运行时端口。

mod action;
mod kind;
mod persistence;

pub use action::{
    DirectoryActionExecutor, DirectoryActionOutput, DirectoryActionRegistry, DirectoryActionRequest,
};
pub use kind::DirectoryKindRegistry;
pub use persistence::{
    DirectoryIndex, DirectoryLocation, DirectoryQuery, DirectoryRepository, LocatedDirectory,
};
