import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ResourceKind } from "@/domain/resource";
import {
  isReadOnlyMetadataSchema,
  KindMetadataPanel,
  metadataKindsForResource,
} from "@/features/resources/components/kind-metadata";
import { resource as resourceFixture } from "./fixtures";

afterEach(cleanup);

const kinds: ResourceKind[] = [
  kind("core:file", [], null),
  kind("core:document", ["core:file"], {
    schemaVersion: 1,
    schema: {
      type: "object",
      title: "Document facts",
      readOnly: true,
      properties: { page_count: { type: "integer", title: "Pages" } },
    },
  }),
  kind("example:book", ["core:document", "core:file"], {
    schemaVersion: 2,
    schema: {
      $schema: "https://json-schema.org/draft/2020-12/schema",
      type: "object",
      title: "Book metadata",
      additionalProperties: false,
      properties: { isbn: { type: "string", title: "ISBN" } },
    },
  }),
];

describe("kind metadata", () => {
  it("orders metadata definitions from root to leaf", () => {
    const resource = {
      ...resourceFixture(),
      kind: "example:book",
    };

    expect(metadataKindsForResource(resource, kinds).map((item) => item.kind)).toEqual([
      "core:file",
      "core:document",
      "example:book",
    ]);
    expect(isReadOnlyMetadataSchema(kinds[1]?.metadata?.schema ?? {})).toBe(true);
    expect(isReadOnlyMetadataSchema(kinds[2]?.metadata?.schema ?? {})).toBe(false);
  });

  it("renders derived ancestor facts and saves or clears an editable leaf layer", async () => {
    const user = userEvent.setup();
    const onPatch = vi.fn(async () => undefined);
    const resource = {
      ...resourceFixture(),
      kind: "example:book",
      metadata: {
        summary: { description: null, tags: [] },
        kindMetadata: {
          layers: [
            { kind: "core:document", schemaVersion: 1, data: { page_count: 320 } },
            { kind: "example:book", schemaVersion: 2, data: { isbn: "old" } },
          ],
        },
      },
    };

    render(
      <KindMetadataPanel resource={resource} kinds={kinds} disabled={false} onPatch={onPatch} />,
    );

    const cards = screen.getAllByRole("article");
    expect(within(cards[0] as HTMLElement).getByText("Document facts")).toBeInTheDocument();
    expect(within(cards[0] as HTMLElement).getByText("320")).toBeInTheDocument();
    expect(within(cards[1] as HTMLElement).getAllByText("Book metadata")).toHaveLength(2);

    const isbn = screen.getByRole("textbox", { name: /isbn/i });
    await user.clear(isbn);
    await user.type(isbn, "new-isbn");
    await user.click(screen.getByRole("button", { name: /save book metadata/i }));
    expect(onPatch).toHaveBeenLastCalledWith({
      upsert: [{ kind: "example:book", schemaVersion: 2, data: { isbn: "new-isbn" } }],
      clear: [],
    });

    await user.click(screen.getByRole("button", { name: /clear layer/i }));
    expect(onPatch).toHaveBeenLastCalledWith({ upsert: [], clear: ["example:book"] });
  });

  it("keeps an outdated layer read-only and allows an explicit reset", async () => {
    const user = userEvent.setup();
    const onPatch = vi.fn(async () => undefined);
    const resource = {
      ...resourceFixture(),
      kind: "example:book",
      metadata: {
        summary: { description: null, tags: [] },
        kindMetadata: {
          layers: [{ kind: "example:book", schemaVersion: 1, data: { legacy_isbn: "old" } }],
        },
      },
    };

    render(
      <KindMetadataPanel resource={resource} kinds={kinds} disabled={false} onPatch={onPatch} />,
    );

    expect(screen.getByText(/remains read-only until it is migrated/i)).toBeInTheDocument();
    expect(screen.getByText(/"legacy_isbn": "old"/i)).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: /isbn/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /save book metadata/i })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /clear outdated layer/i }));
    expect(onPatch).toHaveBeenCalledWith({ upsert: [], clear: ["example:book"] });
  });
});

function kind(name: string, ancestors: string[], metadata: ResourceKind["metadata"]): ResourceKind {
  return {
    kind: name,
    parent: ancestors[0] ?? null,
    ancestors,
    label: name.split(":").at(-1) ?? name,
    supportsContent: true,
    source: "test",
    actions: [],
    detect: null,
    metadata,
  };
}
