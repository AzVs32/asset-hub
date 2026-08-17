# Asset Hub Plugin Web SDK

This package is the stable browser boundary for a Web UI returned as an Asset Hub
`plugin_frame`. It exposes only capabilities that the Host has already bound to the current
Resource or Directory and frame. Penpal is an implementation detail and is bundled into both
browser builds.

The package publishes three stable entry points:

- `@asset-hub/plugin-web-sdk` for frame clients and all public types;
- `@asset-hub/plugin-web-sdk/contract` for transport-free constants and Browser Frame types used
  by Host implementations;
- `@asset-hub/plugin-web-sdk/global` for resolving the self-contained IIFE asset in packaging
  tools.

Bundled applications can import the SDK:

```ts
import { connectAssetHubFrame } from "@asset-hub/plugin-web-sdk";

const host = await connectAssetHubFrame();
const output = await host.executeResourceAction("example.plugin.inspect", { operation: "load" });
```

Both Resource and Directory results contain a nullable `view`, an `effects` array naming the
mutations already applied by the Host, and diagnostics. Effect-only actions therefore return
`view: null`; callers must not assume that every successful Action is renderable.

A plugin with a plain `index.html` can copy `dist/asset-hub-plugin.global.js` into its verified Web
assets and use the global build without React, npm, or another framework:

```html
<script src="./asset-hub-plugin.global.js"></script>
<script>
  AssetHubPlugin.connectAssetHubFrame().then(async (host) => {
    const output = await host.executeResourceAction("example.plugin.inspect", {});
    document.body.textContent = JSON.stringify(output.view);
  });
</script>
```

`executeResourceAction` can call only an Action exposed on the current Resource.
`replaceResourceText` succeeds only for the frame created by the Resource's current write
`text_edit` provider. The Host remains responsible for runtime validation, authorization, content
policy, and optimistic concurrency.

A Directory workspace frame connects through its separate bound client:

```ts
import { connectAssetHubDirectoryFrame } from "@asset-hub/plugin-web-sdk";

const host = await connectAssetHubDirectoryFrame();
const output = await host.executeDirectoryAction("example.game.workspace", {
  operation: "load",
});
await host.refreshDirectory();
await host.navigateToDirectory("games/favorites");
```

`executeDirectoryAction` accepts only an Action exposed on the bound Directory. Refresh and
navigation remain Host operations. A Directory plugin owns its iframe's complete UI and does not
receive the Core workspace's menu, thumbnail, resource-row, or detail slots.

Action input must be a JSON object throughout: nested values may contain only finite numbers,
strings, booleans, null, arrays, and plain objects. The Host rejects cyclic or non-JSON values and
bounds input to 32 nested levels and 10,000 values.

Connection and call timeouts limit how long the SDK waits. They do not cancel work that the Host
has already started. A timed-out write can still complete, so a plugin must refresh authoritative
state before deciding whether another write is safe; it must not blindly retry a mutation.

Build and type-check with Node.js 22.12 or later in the Node.js 22 release line:

```bash
npm ci
npm run typecheck
npm test
npm run build
```
