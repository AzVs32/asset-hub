import { describe, expect, it } from "vitest";
import { parseActionId, parseActionInput, parseResourceId } from "@/plugins/frame-input";

describe("Plugin Frame action input", () => {
  it("accepts only bounded Action IDs", () => {
    expect(parseActionId("example.plugin.action")).toBe("example.plugin.action");
    for (const value of [undefined, "", "a".repeat(129)]) {
      expect(() => parseActionId(value)).toThrow(/Action ID/);
    }
  });

  it("accepts only bounded Resource IDs", () => {
    expect(parseResourceId("resource-id")).toBe("resource-id");
    for (const value of [undefined, "", "r".repeat(129)]) {
      expect(() => parseResourceId(value)).toThrow(/Resource ID/);
    }
  });

  it("accepts a bounded nested JSON object", () => {
    const input = { operation: "load", options: { pages: [1, 2], cached: false, note: null } };

    expect(parseActionInput(input)).toBe(input);
    expect(parseActionInput(undefined)).toEqual({});
  });

  it("rejects values that cannot cross the JSON action boundary", () => {
    const cyclic: Record<string, unknown> = {};
    cyclic.self = cyclic;
    const decoratedArray: unknown[] & { extra?: unknown } = [];
    decoratedArray.extra = () => undefined;
    const tooDeep: Record<string, unknown> = {};
    let cursor = tooDeep;
    for (let depth = 0; depth < 34; depth += 1) {
      const child: Record<string, unknown> = {};
      cursor.child = child;
      cursor = child;
    }

    for (const input of [
      [],
      { nested: undefined },
      { nested: Number.NaN },
      { nested: new Date() },
      { nested: decoratedArray },
      cyclic,
      tooDeep,
    ]) {
      expect(() => parseActionInput(input)).toThrow(/JSON/);
    }
  });
});
