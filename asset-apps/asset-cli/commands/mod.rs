pub(crate) mod config;
pub(crate) mod plugin;
pub(crate) mod system;
pub(crate) mod user;

pub(crate) type CliResult<T = ()> = anyhow::Result<T>;
