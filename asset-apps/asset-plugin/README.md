# Asset Plugin Tool

`asset-plugin` generates and verifies Manifest V2 artifact integrity. Plugin authors do not
calculate or edit SHA-256 values.

Start a plugin directory with a fixed, documented Manifest V2 draft:

```bash
asset-plugin gen manifest
```

The command creates `manifest.json` in the current directory and refuses to overwrite an existing
file. Edit the `example.plugin` metadata, action ID, handler, matching rules, requirements, output,
and permissions for the plugin. It copies the canonical draft from
`asset-plugin-api/templates/manifest.json` byte for byte; the generated draft intentionally omits
integrity hashes.

After building and copying the Wasm and optional Web bundle, run:

```bash
asset-plugin seal path/to/plugin.json
```

Draft manifests may omit `runtime.wasm_sha256` and `web.integrity`. `seal` calculates both,
validates the resulting Manifest V2 contract, preserves the developer-authored semantic fields,
and atomically replaces the JSON file using canonical pretty formatting.

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

The integrity values detect artifact drift and changes to only one part of a package. They do not
replace package signatures: an attacker able to replace both a plugin and its sealed Manifest can
generate matching hashes. Publisher signatures require a separate trust-store design.
