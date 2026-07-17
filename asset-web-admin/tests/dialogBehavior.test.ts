import { describe, expect, it } from "vitest";
import { shouldCloseAfterCreate } from "../src/features/resourceWorkspace/dialogBehavior";

describe("shouldCloseAfterCreate", () => {
  it("keeps the dialog open when creation fails", () => {
    expect(shouldCloseAfterCreate(undefined)).toBe(false);
    expect(shouldCloseAfterCreate(null)).toBe(false);
  });

  it("closes the dialog after successful creation", () => {
    expect(shouldCloseAfterCreate({ path: "docs" })).toBe(true);
  });
});

