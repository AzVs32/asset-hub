//! Real Rust component for host authorization and execution integration tests.
use ah_vfs_storage::{
    BackendFailure, Capabilities, Entry, EntryName, EntryType, Failure, IntermediateSymlinks,
    LinkTarget, Operation, Rejection, StatRequest, StoragePath,
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
            capabilities: Capabilities::default().with_stat(true).with_read_link(true),
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

    fn begin_lookup(&self, _: Operation) -> api::Result<api::Lookup> {
        Ok(api::Lookup::new(LinkLookup))
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

struct LinkLookup;

fn link_failure(failure: Failure) -> api::Failure {
    api::ErrorContext::storage(Operation::ReadLink).failure(failure)
}

impl api::GuestLookup for LinkLookup {
    fn root(&mut self) -> api::Result<(u64, Entry)> {
        let entry = Entry::new(StoragePath::root(), EntryType::Directory, None)
            .map_err(|_| p::Failure::new(p::FailureCode::InvalidRequest))?;
        Ok((1, entry))
    }

    fn observe(&mut self, parent: u64, name: &EntryName) -> api::Result<(u64, Entry)> {
        let (id, path, kind) = match (parent, name.as_str()) {
            (1, "alias") => (2, "alias", EntryType::Symlink),
            (1, "dir") => (3, "dir", EntryType::Directory),
            (3, "link") => (4, "dir/link", EntryType::Symlink),
            (1 | 3, _) => return Err(link_failure(Failure::Rejected(Rejection::NotFound))),
            (2 | 4, _) => return Err(link_failure(Failure::Rejected(Rejection::NotDirectory))),
            _ => return Err(link_failure(Failure::Backend(BackendFailure::Stale))),
        };
        let path = StoragePath::parse(path)
            .map_err(|_| p::Failure::new(p::FailureCode::InvalidRequest))?;
        let entry = Entry::new(path, kind, None)
            .map_err(|_| p::Failure::new(p::FailureCode::InvalidRequest))?;
        Ok((id, entry))
    }

    fn read_link(&self, entry: u64) -> api::Result<LinkTarget> {
        match entry {
            2 => Ok(LinkTarget::new(b"dir".to_vec())),
            4 => Ok(LinkTarget::new(vec![0xff, b'/'])),
            1 | 3 => Err(link_failure(Failure::Rejected(Rejection::NotSymlink))),
            _ => Err(link_failure(Failure::Backend(BackendFailure::Stale))),
        }
    }

    fn verify(&self) -> api::Result<()> {
        Ok(())
    }
}
