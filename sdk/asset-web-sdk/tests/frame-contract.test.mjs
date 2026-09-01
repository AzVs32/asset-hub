import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { describe, it } from "node:test";
import {
  DIRECTORY_FRAME_CHANNEL,
  directoryActionCapabilityIds,
  directoryActionEffectKinds,
  directoryFrameMethods,
  PLUGIN_API_VERSION,
  pluginViewKinds,
  RESOURCE_FRAME_CHANNEL,
  resourceActionCapabilityIds,
  resourceActionEffectKinds,
  resourceFrameMethods,
} from "../dist/contract.js";

const contract = JSON.parse(
  readFileSync(new URL("../../../asset-plugin-api/spec/contract-v4.json", import.meta.url)),
);

describe("Browser Frame contract", () => {
  it("matches the Rust Plugin API golden", () => {
    assert.equal(PLUGIN_API_VERSION, contract.plugin_api);
    assert.equal(RESOURCE_FRAME_CHANNEL, contract.browser_frames.resource.channel);
    assert.equal(DIRECTORY_FRAME_CHANNEL, contract.browser_frames.directory.channel);
    assert.deepEqual(
      resourceFrameMethods,
      contract.browser_frames.resource.host_methods.map(({ name }) => name),
    );
    assert.deepEqual(
      directoryFrameMethods,
      contract.browser_frames.directory.host_methods.map(({ name }) => name),
    );
    assert.deepEqual(pluginViewKinds, contract.views);
    assert.deepEqual(resourceActionEffectKinds, contract.resource_effects);
    assert.deepEqual(directoryActionEffectKinds, contract.directory_effects);
  });
});

describe("Manifest capability contract", () => {
  it("matches the Rust Manifest v5 catalog", () => {
    assert.equal(contract.manifest_version, 5);
    assert.deepEqual(resourceActionCapabilityIds, contract.capabilities.resource_actions);
    assert.deepEqual(directoryActionCapabilityIds, contract.capabilities.directory_actions);
  });
});
