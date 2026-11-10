//! Real Rust component for host authorization and execution integration tests.
use ah_vfs_storage::{
    Capabilities, Entry, EntryType, Failure, IntermediateSymlinks, Operation, Rejection,
    StatRequest, StoragePath,
};
use std::num::NonZeroUsize;
use vfs_plugin_guest::api::{self, Description, Guest, GuestStorage};
use vfs_plugin_protocol as p;

vfs_plugin_guest::export_plugin!(Probe, "../../../../vfs-plugin-protocol/wit");
struct Probe;
fn denied<T>(_error: T) -> p::Failure {
    p::Failure::new(p::FailureCode::LifecycleAccessDenied)
}
fn field<'a>(fields: &'a p::JsonObject, key: &str) -> Result<&'a str, p::Failure> {
    fields
        .get(key)
        .and_then(|value| value.as_str())
        .ok_or_else(|| p::Failure::new(p::FailureCode::InvalidRequest))
}
fn run(fields: &p::JsonObject) -> Result<String, p::Failure> {
    use std::{io::Read, net::ToSocketAddrs};
    Ok(match field(fields, "command")? {
        "env" => std::env::var(field(fields, "name")?).unwrap_or_else(|_| "absent".into()),
        "args" => std::env::args().count().to_string(),
        "read-file" => std::fs::read_to_string(field(fields, "path")?).map_err(denied)?,
        "write-file" => {
            std::fs::write(field(fields, "path")?, field(fields, "data")?).map_err(denied)?;
            "written".into()
        }
        "tcp" => {
            let mut stream =
                std::net::TcpStream::connect(field(fields, "address")?).map_err(denied)?;
            let mut byte = [0];
            stream.read_exact(&mut byte).map_err(denied)?;
            String::from_utf8_lossy(&byte).into_owned()
        }
        "dns" => {
            let addrs = (field(fields, "host")?, 80)
                .to_socket_addrs()
                .map_err(denied)?
                .collect::<Vec<_>>();
            format!("{addrs:?}")
        }
        "rebind" => {
            let host = field(fields, "host")?;
            let port = fields
                .get("port")
                .and_then(|value| value.as_u64())
                .unwrap_or(80) as u16;
            let first = (host, port)
                .to_socket_addrs()
                .map_err(denied)?
                .next()
                .ok_or_else(|| denied(()))?;
            assert_eq!(first.ip().to_string(), "200.0.0.1");
            let second = (host, port)
                .to_socket_addrs()
                .map_err(denied)?
                .next()
                .ok_or_else(|| denied(()))?;
            let _stream = std::net::TcpStream::connect(second).map_err(denied)?;
            "unexpected connection".into()
        }
        "http" => {
            let mut request = http::Request::builder()
                .method(
                    fields
                        .get("method")
                        .and_then(|value| value.as_str())
                        .unwrap_or("GET"),
                )
                .uri(field(fields, "url")?);
            for i in 0..fields
                .get("headers")
                .and_then(|value| value.as_u64())
                .unwrap_or(0)
            {
                request = request.header(format!("x-probe-{i}"), "value");
            }
            let body = vec![
                b'x';
                fields
                    .get("bytes")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0) as usize
            ];
            let response = vfs_plugin_guest::wasi_http::send(request.body(body).map_err(denied)?)
                .map_err(|cause| {
                p::Failure::from(vfs_plugin_guest::api::ErrorContext::lifecycle().failure(cause))
            })?;
            String::from_utf8_lossy(response.body()).into_owned()
        }
        "cpu" => loop {
            std::hint::spin_loop();
        },
        "memory" => {
            #[cfg(target_arch = "wasm32")]
            {
                let _ = core::arch::wasm32::memory_grow::<0>(8192);
            }
            "grown".into()
        }
        "clock-random" => {
            use std::hash::{BuildHasher, Hasher};
            let random = std::collections::hash_map::RandomState::new()
                .build_hasher()
                .finish();
            format!(
                "{}:{random}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(denied)?
                    .as_secs()
            )
        }
        "echo" => field(fields, "data")?.into(),
        _ => return Err(p::Failure::new(p::FailureCode::InvalidRequest)),
    })
}
impl Guest for Probe {
    fn describe() -> Description {
        Description {
            key: "wasi.probe".into(),
            name: "WASI integration probe".into(),
            version: "1.0.0".into(),
            child_mounts: false,
            capabilities: Capabilities::default().with_stat(true),
            required_features: vec![],
            required_host_capabilities: vec![],
            settings_schema: None,
            instance_schema: None,
            config_version: 1,
        }
    }

    fn initialize(_: p::Object) -> api::Result<()> {
        Ok(())
    }

    fn prepare(config: p::Object) -> api::Result<p::Prepared> {
        let result = run(&p::json_object(config.clone())?)?;
        Ok(p::Prepared {
            config: Some(config),
            details: Some(p::Object::from([("result".into(), result.into())])),
        })
    }

    fn migrate(_: u32, config: p::Object) -> api::Result<p::Object> {
        Ok(config)
    }

    fn test_connection(_: p::Object) -> api::Result<()> {
        Ok(())
    }

    fn open(_: String, _: p::Object) -> api::Result<api::Storage> {
        Ok(api::Storage::new(Probe))
    }
}

impl GuestStorage for Probe {
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::new(1024).unwrap_or(NonZeroUsize::MIN)
    }

    fn stat(
        &self,
        request: &StatRequest,
        base: &StoragePath,
        links: IntermediateSymlinks,
    ) -> api::Result<Entry> {
        api::direct_stat(request, base, links, |path| {
            let context = api::ErrorContext::storage(Operation::Stat);
            if !path.is_root() {
                return Err(context.failure(Failure::Rejected(Rejection::NotFound)));
            }
            Entry::new(path.clone(), EntryType::Directory, None).map_err(|_| {
                context.failure(Failure::Contract(
                    ah_vfs_storage::ContractViolation::InvalidEntry,
                ))
            })
        })
    }
}
