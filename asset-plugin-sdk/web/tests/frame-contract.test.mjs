import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { describe, it } from "node:test";
import {
  DIRECTORY_FRAME_CHANNEL,
  directoryActionEffectKinds,
  PLUGIN_API_VERSION,
  pluginViewKinds,
  RESOURCE_FRAME_CHANNEL,
  resourceActionEffectKinds,
} from "../dist/contract.js";

const fixture = JSON.parse(
  readFileSync(new URL("../../tests/fixtures/plugin-frame-contract-v1.json", import.meta.url)),
);

describe("Browser Frame contract", () => {
  it("matches the Rust Plugin API golden", () => {
    assert.equal(PLUGIN_API_VERSION, fixture.plugin_api);
    assert.equal(RESOURCE_FRAME_CHANNEL, fixture.channels.resource);
    assert.equal(DIRECTORY_FRAME_CHANNEL, fixture.channels.directory);
    assert.deepEqual(pluginViewKinds, fixture.view_kinds);
    assert.deepEqual(resourceActionEffectKinds, fixture.resource_effect_kinds);
    assert.deepEqual(directoryActionEffectKinds, fixture.directory_effect_kinds);
  });
});
