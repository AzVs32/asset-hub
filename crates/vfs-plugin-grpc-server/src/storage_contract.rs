//! An authenticated, bounded memory-storage session using the new domain contract.
//! The service is independent of plugin registration and binds one configured
//! instance.
use crate::{ServerLimits, limits, security::AuthToken};
use ah_vfs::application::driver_error::DriverError;
use ah_vfs_storage as d;
use std::{pin::Pin, sync::Arc};
use tokio::sync::mpsc;
use tokio_stream::{Stream, wrappers::ReceiverStream};
use tonic::{Request, Response, Status};
use vfs_driver_memory::MemoryStorage;
pub use vfs_plugin_grpc_wire::storage_contract as wire;
use wire::{command::Action, reply::Result as Reply};

pub fn service(
    storage: Arc<MemoryStorage>,
    token: &str,
    limits: ServerLimits,
) -> Result<wire::storage_contract_server::StorageContractServer<ContractService>, DriverError> {
    Ok(
        wire::storage_contract_server::StorageContractServer::new(ContractService {
            storage,
            token: AuthToken::new(token)?,
            admission: Arc::new(limits::Admission::new(limits)?),
        })
        .max_decoding_message_size(wire::MAX_FRAME_BYTES)
        .max_encoding_message_size(wire::MAX_FRAME_BYTES),
    )
}

pub struct ContractService {
    storage: Arc<MemoryStorage>,
    token: AuthToken,
    admission: Arc<limits::Admission>,
}

#[tonic::async_trait]
impl wire::storage_contract_server::StorageContract for ContractService {
    type SessionStream = Pin<Box<dyn Stream<Item = Result<wire::Reply, Status>> + Send>>;

    async fn session(
        &self,
        request: Request<tonic::Streaming<wire::Command>>,
    ) -> Result<Response<Self::SessionStream>, Status> {
        let permit = Arc::new(self.admission.acquire()?);
        if !self.token.accepts(request.metadata()) {
            return Err(Status::unauthenticated("storage authentication failed"));
        }
        let mut incoming = request.into_inner();
        let (tx, rx) = mpsc::channel(1);
        let mut session = Session {
            storage: self.storage.clone(),
        };
        let admission = self.admission.clone();
        let task_permit = permit.clone();
        tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    () = tx.closed() => break,
                    frame = incoming.message() => frame,
                };
                let response = match frame {
                    Ok(Some(frame)) => {
                        if let Err(status) = admission.operation() {
                            let _ =
                                send(&tx, admission.limits.stream_idle_timeout, Err(status)).await;
                            break;
                        }
                        let in_flight_permit = task_permit.clone();
                        let operation = tokio::task::spawn_blocking(move || {
                            let _permit = in_flight_permit;
                            let response = session.call(frame);
                            (session, response)
                        });
                        let result = tokio::select! {
                            () = tx.closed() => break,
                            result = tokio::time::timeout(admission.limits.request_timeout, operation) => result,
                        };
                        match result {
                            Ok(Ok((state, response))) => {
                                session = state;
                                response
                            }
                            failure => {
                                // Dropping the join handle can leave a dispatched
                                // operation running. Its state and permit stay
                                // with the worker until it completes.
                                let status = if failure.is_err() {
                                    Status::deadline_exceeded("storage operation timed out")
                                } else {
                                    Status::internal("storage operation failed")
                                };
                                let _ =
                                    send(&tx, admission.limits.stream_idle_timeout, Err(status))
                                        .await;
                                break;
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(status) => Err(status),
                };
                let close = response.is_err();
                if !send(&tx, admission.limits.stream_idle_timeout, response).await || close {
                    break;
                }
            }
            // Dropping the session drops the writer and releases its quota.
        });
        Ok(Response::new(Box::pin(limits::SessionStream {
            inner: ReceiverStream::new(rx),
            permit: Some(permit),
        })))
    }
}

async fn send(
    tx: &mpsc::Sender<Result<wire::Reply, Status>>,
    timeout: std::time::Duration,
    response: Result<wire::Reply, Status>,
) -> bool {
    matches!(
        tokio::time::timeout(timeout, tx.send(response)).await,
        Ok(Ok(()))
    )
}

struct Session {
    storage: Arc<MemoryStorage>,
}

enum CallError {
    Input(Status),
    Storage(d::StorageError),
}

impl From<Status> for CallError {
    fn from(status: Status) -> Self {
        Self::Input(status)
    }
}

impl From<d::StorageError> for CallError {
    fn from(error: d::StorageError) -> Self {
        Self::Storage(error)
    }
}

impl Session {
    fn call(&mut self, frame: wire::Command) -> Result<wire::Reply, Status> {
        let result = match self.execute(frame) {
            Ok(value) => value,
            Err(CallError::Input(status)) => return Err(status),
            Err(CallError::Storage(error)) => Reply::Failure(
                wire::encode_failure(&error)
                    .map_err(|_| Status::internal("unrepresentable storage failure"))?,
            ),
        };
        Ok(wire::Reply {
            result: Some(result),
        })
    }

    fn execute(&mut self, frame: wire::Command) -> Result<Reply, CallError> {
        let links = if frame.reject_intermediate_links {
            d::IntermediateSymlinks::Reject
        } else {
            d::IntermediateSymlinks::FollowWithinInstance
        };
        let unit = || Reply::Unit(wire::Unit {});
        match frame.action.ok_or_else(invalid_request)? {
            Action::CreateDirectory(request) => {
                let mode = if request.recursive {
                    d::CreateDirectoryMode::Recursive
                } else {
                    d::CreateDirectoryMode::Single
                };
                let request = d::CreateDirectoryRequest::new(path(&request.path)?, mode);
                entry(
                    d::Operation::CreateDirectory,
                    self.storage.create_directory(&request, links)?,
                )
            }
            Action::Remove(request) => {
                let kind = match wire::RemoveKind::try_from(request.kind) {
                    Ok(wire::RemoveKind::NonDirectory) => d::RemoveKind::NonDirectory,
                    Ok(wire::RemoveKind::EmptyDirectory) => d::RemoveKind::EmptyDirectory,
                    _ => return Err(invalid_request().into()),
                };
                let request = d::RemoveRequest::new(path(&request.path)?, kind)
                    .map_err(|_| invalid_request())?;
                self.storage.remove(&request, links)?;
                Ok(unit())
            }
            Action::GetPermissions(request) => {
                let request = d::AttributeRequest::new(
                    path(&request.path)?,
                    d::AttributeAction::GetPermissions,
                );
                let op = request.operation();
                Err(reject(op, d::Rejection::Unsupported).into())
            }
        }
    }
}

fn entry(op: d::Operation, value: d::Entry) -> Result<Reply, CallError> {
    wire::encode_entry(value)
        .map(Reply::Entry)
        .map_err(|cause| response_error(op, cause).into())
}
fn response_error(op: d::Operation, cause: d::ContractViolation) -> d::StorageError {
    d::StorageError::after_dispatch(op, d::Failure::Contract(cause), None)
}
fn reject(op: d::Operation, cause: d::Rejection) -> d::StorageError {
    d::StorageError::without_effects(op, d::Failure::Rejected(cause), None)
}

fn invalid_request() -> Status {
    Status::invalid_argument("invalid storage command")
}
fn path(input: &str) -> Result<d::PathExpr, Status> {
    d::PathExpr::parse(input).map_err(|_| invalid_request())
}
