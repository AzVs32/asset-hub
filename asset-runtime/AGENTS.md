# asset-runtime instructions

## Responsibility

`asset-runtime` assembles initialized infrastructure and Core services for application surfaces.
It owns background-task/guard lifetimes that must survive for the running application.

It is reusable by current and future application surfaces; therefore it MUST remain independent of
transport routing, argument parsing, presentation logic, and surface-specific lifecycle policy.

## Rules

MUST:

- accept already-loaded infrastructure configuration from the caller;
- construct `AssetInfrastructure` and expose stable Core services/registries needed by application
  surfaces;
- explicitly own long-lived synchronization guards/tasks;
- keep startup ordering deterministic, including upload-finalization recovery;
- make optional background services idempotent to start when the runtime contract promises that
  behavior.

MUST NOT:

- resolve configuration sources or parse surface-specific input;
- choose transport, authentication-session, presentation, or interaction policy owned by an
  application surface;
- contain business workflows that belong in `asset-core::service`;
- expose more concrete infrastructure than callers genuinely need;
- spawn detached critical tasks whose lifetime or failure cannot be observed/owned.

## Backend evolution

The runtime currently exposes SQLite-specific pool access for a caller-owned session store. Backend
expansion therefore requires an intentional abstraction for consumers of that concrete pool; adding
only another database migration or repository is incomplete.

## Tests

Test construction, service exposure, repeated background-start behavior, and cleanup/lifetime
behavior when runtime ownership changes.

Run:

```bash
cargo test -p asset-runtime
```
