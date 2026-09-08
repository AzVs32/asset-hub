# asset-core

`asset-core` is Asset Hub's framework-independent domain and application kernel. It defines no
Axum, SQLx, OpenDAL, filesystem, or CLI types.

## Module boundaries

- `directory`: the Directory aggregate, its persistence/query ports, and directory application
  services.
- `resource`: the Resource aggregate, resource-content and upload state, resource ports, and
  resource application services.
- `idempotency`: durable request-idempotency domain types, repository port, and lease service.
- `storage`: ports for blob bytes, physical directories, health checks, and storage scans.
- `workflow`: cross-aggregate application workflows; it does not own a domain aggregate or a
  storage adapter port.
- `error`: Core-wide and aggregate-specific error types.
- `utils`: crate-private shared implementation utilities.

Within an aggregate boundary, `domain`, `port`, and `service` stay together. Consumers should use
the capability-scoped paths, for example `asset_core::resource::service::ResourceService` and
`asset_core::directory::port::DirectoryStore`, rather than a crate-wide domain, port, or service
namespace.
