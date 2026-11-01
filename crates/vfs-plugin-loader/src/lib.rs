//! Configuration-driven external plugin loading. A host assembles built-ins once,
//! then hands its catalog to this loader. Only transport kinds are selected here;
//! driver names, instance fields and backend protocols are never enumerated.
//!
//! ```toml
//! [[vfs.plugins.entries]]
//! transport = "grpc"
//! endpoint = "https://plugin.example:7443"
//! token_env = "VFS_REMOTE_TOKEN"
//!
//! [[vfs.plugins.entries]]
//! transport = "wasm"
//! module = "plugins/object-storage.wasm"
//! profile = "assethub.service.v1"
//! [vfs.plugins.entries.grants]
//! http_origins = ["https://objects.example"]
//! ```
//!
//! Instance configuration is supplied later through InstanceService. Bootstrap
//! paths resolve relative to the host configuration file, not the process cwd.
mod config;
pub use config::{GrpcTls, PluginConfig, PluginSource, PluginSpec};

use ah_vfs::{
    application::{driver_catalog::DriverCatalog, error::DriverCatalogError, port::Driver},
    domain::{DriverError, DriverKey},
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;
use vfs_plugin_grpc::{Certificate, ClientTlsConfig, GrpcTransport, Identity};
use vfs_plugin_host::{PluginDriver, Transport};
use vfs_plugin_wasm::WasmTransport;

#[derive(Debug, Error)]
pub enum LoadFailure {
    #[error("invalid plugin bootstrap configuration")]
    InvalidConfiguration,
    #[error("plugin transport credential is unavailable")]
    CredentialUnavailable,
    #[error("could not read a plugin bootstrap file")]
    File(#[source] std::io::Error),
    #[error("plugin connection or declaration failed")]
    Driver(#[source] DriverError),
    #[error("plugin registration failed")]
    Registration(#[source] DriverCatalogError),
}

#[derive(Debug, Error)]
#[error("plugin entry {index} failed: {failure}")]
pub struct PluginLoadError {
    /// Zero-based configuration entry; diagnostics never print settings/tokens.
    pub index: usize,
    #[source]
    pub failure: LoadFailure,
}

pub struct PluginLoader {
    base_dir: PathBuf,
}
impl PluginLoader {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    pub fn from_loaded(config: &ah_config::LoadedConfig) -> Self {
        Self::new(config.path().parent().unwrap_or_else(|| Path::new(".")))
    }

    /// Load the common registered namespace and return a ready catalog containing
    /// both supplied built-ins and all configured external plugins.
    pub async fn load_from(
        config: &ah_config::LoadedConfig,
        builtins: DriverCatalog,
    ) -> Result<DriverCatalog, BootstrapError> {
        let plugins = config
            .get::<PluginConfig>()
            .map_err(BootstrapError::Configuration)?;
        Self::from_loaded(config)
            .load_catalog(&plugins, builtins)
            .await
            .map_err(BootstrapError::Plugin)
    }

    /// The catalog is consumed so failure cannot return a partially composed
    /// catalog to callers. External gRPC service supervision remains independent
    /// of registration; stopping a remote process is not implied by dropping it.
    pub async fn load_catalog(
        &self,
        config: &PluginConfig,
        mut catalog: DriverCatalog,
    ) -> Result<DriverCatalog, PluginLoadError> {
        for (index, spec) in config.entries.iter().enumerate() {
            self.register(spec, &mut catalog)
                .await
                .map_err(|failure| PluginLoadError { index, failure })?;
        }
        Ok(catalog)
    }

    async fn register(
        &self,
        spec: &PluginSpec,
        catalog: &mut DriverCatalog,
    ) -> Result<(), LoadFailure> {
        let expected = spec
            .expected_key
            .as_deref()
            .map(|key| DriverKey::parse(key).map_err(|_| LoadFailure::InvalidConfiguration))
            .transpose()?;
        if expected
            .as_ref()
            .is_some_and(|key| catalog.get(key).is_some())
        {
            return Err(LoadFailure::InvalidConfiguration);
        }
        let transport: Arc<dyn Transport> = match &spec.source {
            PluginSource::Grpc {
                endpoint,
                token_env,
                tls,
            } => {
                GrpcTransport::validate_endpoint(endpoint)
                    .map_err(|_| LoadFailure::InvalidConfiguration)?;
                let url =
                    url::Url::parse(endpoint).map_err(|_| LoadFailure::InvalidConfiguration)?;
                if tls.is_some() && url.scheme() != "https" {
                    return Err(LoadFailure::InvalidConfiguration);
                }
                let token =
                    std::env::var(token_env).map_err(|_| LoadFailure::CredentialUnavailable)?;
                if token.is_empty() {
                    return Err(LoadFailure::CredentialUnavailable);
                }
                let tls = match tls {
                    Some(tls) => Some(self.tls(tls).await?),
                    None => None,
                };
                Arc::new(
                    GrpcTransport::connect_with_tls(endpoint.clone(), &token, tls)
                        .await
                        .map_err(LoadFailure::Driver)?,
                )
            }
            PluginSource::Wasm {
                module,
                profile,
                grants,
            } => {
                if profile != vfs_plugin_protocol::WASM_HOST_PROFILE {
                    return Err(LoadFailure::InvalidConfiguration);
                }
                let bytes = self.read(module).await?;
                Arc::new(
                    WasmTransport::load_with_grants(bytes, grants.clone())
                        .await
                        .map_err(LoadFailure::Driver)?,
                )
            }
        };
        let driver = Arc::new(
            PluginDriver::connect(transport)
                .await
                .map_err(LoadFailure::Driver)?,
        );
        if expected
            .as_ref()
            .is_some_and(|key| key != driver.info().key())
        {
            return Err(LoadFailure::InvalidConfiguration);
        }
        catalog
            .register(driver, spec.settings.clone().into())
            .await
            .map_err(LoadFailure::Registration)
    }

    async fn read(&self, path: &Path) -> Result<Vec<u8>, LoadFailure> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base_dir.join(path)
        };
        tokio::fs::read(path).await.map_err(LoadFailure::File)
    }

    async fn tls(&self, config: &GrpcTls) -> Result<ClientTlsConfig, LoadFailure> {
        let mut tls = ClientTlsConfig::new().with_enabled_roots();
        if let Some(path) = &config.ca_certificate {
            tls = tls.ca_certificate(Certificate::from_pem(self.read(path).await?));
        }
        match (&config.client_certificate, &config.client_key) {
            (Some(cert), Some(key)) => {
                tls = tls.identity(Identity::from_pem(
                    self.read(cert).await?,
                    self.read(key).await?,
                ));
            }
            (None, None) => {}
            _ => return Err(LoadFailure::InvalidConfiguration),
        }
        if let Some(name) = &config.server_name {
            tls = tls.domain_name(name.clone());
        }
        Ok(tls)
    }
}

#[derive(Debug, Error)]
pub enum BootstrapError {
    #[error("plugin configuration is unavailable")]
    Configuration(#[source] ah_config::ConfigError),
    #[error(transparent)]
    Plugin(#[from] PluginLoadError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn insecure_endpoints_are_rejected_before_reading_credentials() {
        for endpoint in [
            "http://192.0.2.1:7443",
            "http://plugin.example:7443",
            "http://localhost:7443",
        ] {
            let spec = PluginSpec {
                expected_key: None,
                source: PluginSource::Grpc {
                    endpoint: endpoint.into(),
                    token_env: String::new(),
                    tls: None,
                },
                settings: Default::default(),
            };
            let error = PluginLoader::new("/tmp")
                .register(&spec, &mut DriverCatalog::new())
                .await;
            assert!(matches!(error, Err(LoadFailure::InvalidConfiguration)));
        }
        for endpoint in [
            "http://127.0.0.1:7443",
            "http://[::1]:7443",
            "https://plugin.example:7443",
        ] {
            let spec = PluginSpec {
                expected_key: None,
                source: PluginSource::Grpc {
                    endpoint: endpoint.into(),
                    token_env: String::new(),
                    tls: None,
                },
                settings: Default::default(),
            };
            let error = PluginLoader::new("/tmp")
                .register(&spec, &mut DriverCatalog::new())
                .await;
            assert!(matches!(error, Err(LoadFailure::CredentialUnavailable)));
        }
    }
}
