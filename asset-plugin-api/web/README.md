# Asset Hub Plugin Web SDK

This package is the stable browser boundary for a Web UI returned as an Asset Hub
`plugin_frame`. It exposes only capabilities that the Host has already bound to the current
Resource and frame. Penpal is an implementation detail and is bundled into both browser builds.

Bundled applications can import the SDK:

```ts
import { connectAssetHubFrame } from "@asset-hub/plugin-web-sdk";

const host = await connectAssetHubFrame();
const output = await host.executeResourceAction("example.plugin.inspect", { operation: "load" });
```

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

Build and type-check with Node.js 22:

```bash
npm ci
npm run typecheck
npm run build
```
