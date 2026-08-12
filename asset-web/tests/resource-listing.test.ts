import { describe, expect, it } from "vitest";
import type { ResourceFilters } from "@/domain/resource";
import { searchParamsForFilters } from "@/features/resources/hooks/use-resource-listing";

describe("resource listing URL state", () => {
  it("clears selections when list filters change", () => {
    const filters: ResourceFilters = {
      directory: "books",
      page: 1,
      limit: 30,
      query: "",
      kind: "",
      includeDeleted: false,
    };
    const current = new URLSearchParams({
      resource: "resource-1",
      folder: "directory-1",
      unrelated: "kept",
    });

    const next = searchParamsForFilters(current, filters, { page: 2, query: "epub" });

    expect(next.get("page")).toBe("2");
    expect(next.get("q")).toBe("epub");
    expect(next.get("resource")).toBeNull();
    expect(next.get("folder")).toBeNull();
    expect(next.get("unrelated")).toBe("kept");
  });
});
