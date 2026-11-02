//! Real Rust component for host authorization and execution integration tests.
use vfs_plugin_protocol::{self as p, Operation as Op, Value};
wit_bindgen::generate!({
    path: "../../../../vfs-plugin-protocol/wit",
    world: "plugin",
});
struct Probe;
fn denied<T>(_error: T) -> p::Failure {
    p::Failure::new(p::FailureCode::PermissionDenied)
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
            let response = vfs_plugin_guest::wasi_http::send(request.body(body).map_err(denied)?)?;
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
impl exports::assethub::vfs::driver::Guest for Probe {
    fn call(request: Vec<u8>) -> Vec<u8> {
        let reply = (|| {
            let (_, op, _) = p::decode_request(&request)
                .map_err(|_| p::Failure::new(p::FailureCode::InvalidRequest))?;
            let result = match op {
                Op::Describe(_) => Value::Description(
                    p::Description {
                        major: p::PROTOCOL_MAJOR,
                        minor: p::PROTOCOL_MINOR,
                        instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
                        key: "wasi.probe".into(),
                        name: "WASI integration probe".into(),
                        version: "1.0.0".into(),
                        config_version: 1,
                        capabilities: vec![p::Capability::Stat as i32],
                        ..Default::default()
                    }
                    .into(),
                ),
                Op::Initialize(_) => Value::Unit(p::Empty {}),
                Op::Prepare(p::Config {
                    config: Some(config),
                }) => {
                    let result = run(&p::json_object(config.clone())?)?;
                    Value::Prepared(p::Prepared {
                        config: Some(config),
                        details: Some(p::Object::from([("result".into(), result.into())])),
                    })
                }
                Op::Open(_) => Value::Storage(p::Storage {
                    handle: 1,
                    info: Some(p::StorageInfo {
                        capabilities: vec![p::Capability::Stat as i32],
                        chunk_size: 1024,
                    }),
                }),
                Op::Storage(p::StorageCall {
                    operation: Some(p::StorageOp::Stat(_)),
                    ..
                }) => Value::Entry(p::Entry {
                    path: String::new(),
                    kind: p::EntryKind::Dir as i32,
                    size: None,
                    modified: None,
                }),
                Op::Release(_) => Value::Unit(p::Empty {}),
                _ => return Err(p::Failure::new(p::FailureCode::UnsupportedOperation)),
            };
            Ok(p::Reply::new(result))
        })();
        p::encode_reply(reply).expect("encode reply")
    }
}
export!(Probe);
