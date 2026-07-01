# AzVs Markdown Plugin

Extism plugin for Asset Hub Markdown document processing.

## Files

- `azvs-markdown.json`: plugin manifest.
- `src/index.ts`: Extism action entrypoint.
- `src/markdown.ts`: Markdown parser and platform-neutral view model builder.
- `azvs-markdown.wasm`: compiled plugin output, produced by `npm run build`.

## Contract

- Plugin ID: `azvs.markdown`
- Parent kind: `core:document`
- Action: `azvs:render_markdown`
- Output view: `markdown`

The action returns an Asset Hub `PluginView` Markdown payload:

```json
{
  "view": "markdown",
  "markdown": "# Title\n\nBody text."
}
```

The parser also builds a structured Markdown description internally. The plugin
currently returns the original Markdown so existing clients can display it
directly.

## Build

Use the repository's Node toolchain if Node is not on PATH:

```bash
PATH=/storage/apps/node-v22.20.0/bin:$PATH npm install
PATH=/storage/apps/node-v22.20.0/bin:$PATH npm run build
```

The build requires the external `extism-js` compiler and Binaryen tools on
`PATH`, as required by the Extism JavaScript PDK.

Then add `plugins/azvs-markdown/azvs-markdown.json` to
`kind.plugin_manifests` in `config.toml`.
