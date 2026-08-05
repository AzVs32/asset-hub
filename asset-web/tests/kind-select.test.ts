import { describe, expect, it } from "vitest";
import { kindTreeOptions } from "@/features/resources/components/kind-select";

describe("kind tree options", () => {
  it("orders flat kind definitions as an arbitrary-depth tree", () => {
    const options = kindTreeOptions([
      { kind: "azvs:markdown", label: "Markdown", parent: "core:text" },
      { kind: "core:resource", label: "Resource", parent: null },
      { kind: "core:image", label: "Image", parent: "core:resource" },
      { kind: "core:text", label: "Text", parent: "core:resource" },
    ]);

    expect(options.map(({ item }) => item.kind)).toEqual([
      "core:resource",
      "core:image",
      "core:text",
      "azvs:markdown",
    ]);
    expect(options.map(({ prefix }) => prefix)).toEqual([
      "",
      "\u00a0\u00a0\u00a0",
      "\u00a0\u00a0\u00a0",
      "\u00a0\u00a0\u00a0\u00a0\u00a0\u00a0",
    ]);
  });

  it("keeps definitions with an unavailable parent visible as roots", () => {
    const options = kindTreeOptions([
      { kind: "plugin:asset", label: "Plugin asset", parent: "missing:parent" },
    ]);

    expect(options).toEqual([
      {
        item: { kind: "plugin:asset", label: "Plugin asset", parent: "missing:parent" },
        prefix: "",
      },
    ]);
  });
});
