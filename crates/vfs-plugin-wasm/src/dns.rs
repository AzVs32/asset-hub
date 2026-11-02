//! Restrict the standard WASI DNS interface before issuing any lookup.
use super::Host;
use wasmtime::component::Resource;
use wasmtime_wasi::{
    p2::{
        SocketError,
        bindings::sockets::{
            ip_name_lookup as dns,
            network::{ErrorCode, IpAddress, Network},
        },
    },
    sockets::WasiSocketsView,
};
use wasmtime_wasi_io::poll::DynPollable;
// These resource types are the standard WASI resources, not an adapter protocol.
impl dns::Host for Host {
    fn resolve_addresses(
        &mut self,
        network: Resource<Network>,
        name: String,
    ) -> Result<Resource<dns::ResolveAddressStream>, SocketError> {
        if !self.network.allows_name(&name) {
            return Err(ErrorCode::AccessDenied.into());
        }
        dns::Host::resolve_addresses(&mut self.sockets(), network, name)
    }
}
impl dns::HostResolveAddressStream for Host {
    fn resolve_next_address(
        &mut self,
        resource: Resource<dns::ResolveAddressStream>,
    ) -> Result<Option<IpAddress>, SocketError> {
        dns::HostResolveAddressStream::resolve_next_address(&mut self.sockets(), resource)
    }
    fn subscribe(
        &mut self,
        resource: Resource<dns::ResolveAddressStream>,
    ) -> wasmtime::Result<Resource<DynPollable>> {
        dns::HostResolveAddressStream::subscribe(&mut self.sockets(), resource)
    }
    async fn drop(
        &mut self,
        resource: Resource<dns::ResolveAddressStream>,
    ) -> wasmtime::Result<()> {
        dns::HostResolveAddressStream::drop(&mut self.sockets(), resource).await
    }
}

impl wasmtime_wasi::p2::bindings::sockets::network::Host for Host {
    fn convert_error_code(&mut self, error: SocketError) -> wasmtime::Result<ErrorCode> {
        error.downcast()
    }
    fn network_error_code(
        &mut self,
        error: Resource<wasmtime::Error>,
    ) -> wasmtime::Result<Option<ErrorCode>> {
        wasmtime_wasi::p2::bindings::sockets::network::Host::network_error_code(
            &mut self.sockets(),
            error,
        )
    }
}
impl wasmtime_wasi::p2::bindings::sockets::network::HostNetwork for Host {
    fn drop(&mut self, resource: Resource<Network>) -> wasmtime::Result<()> {
        wasmtime_wasi::p2::bindings::sockets::network::HostNetwork::drop(
            &mut self.sockets(),
            resource,
        )
    }
}
