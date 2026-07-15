# Asset Plugin Tool

`asset-plugin` generates and verifies Manifest V2 artifact integrity. Plugin authors do not
calculate or edit SHA-256 values.

Manifest V2 parsing is strict: unknown fields at any protocol level are rejected. This includes
misspelled optional fields, so `verify` should be part of every plugin release pipeline.

Start a plugin directory with a fixed, documented Manifest V2 draft:

```bash
asset-plugin gen manifest
```

The command creates `manifest.json` in the current directory and refuses to overwrite an existing
file. Edit the `example.plugin` metadata, action ID, handler, matching rules, requirements, views,
and permissions for the plugin. It copies the canonical draft from
`asset-plugin-api/templates/manifest.json` byte for byte. Integrity hashes belong to the generated
lock file, not the editable manifest.

After building and copying the Wasm and optional Web bundle, run:

```bash
asset-plugin seal path/to/plugin.json
```

`seal` calculates the Wasm digest and optional Web asset integrity map, validates the Manifest V2
contract, preserves the developer-authored manifest, and atomically writes a sibling
`manifest.lock.json`.

Release and CI environments must not reseal changed artifacts. They verify the previously sealed
package instead:

```bash
asset-plugin verify path/to/plugin.json
```

Build pipelines that produce Wasm and Web in separate jobs can use `verify-wasm` and `verify-web`.
These scoped commands are read-only.

From an Asset Hub checkout, install the command with:

```bash
cargo install --path asset-apps --bin asset-plugin
```

The lock values detect artifact drift and changes to only one part of a package. They do not
replace package signatures: an attacker able to replace a plugin and its lock file can generate
matching hashes. Publisher signatures require a separate trust-store design.
