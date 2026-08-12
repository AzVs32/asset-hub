import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { DirectoryKind } from "@/domain/resource";
import {
  DirectoryBreadcrumbs,
  DirectoryKindEditor,
} from "@/features/resources/components/directory-navigation";
import { directory } from "./fixtures";

afterEach(cleanup);

const kinds: DirectoryKind[] = [
  {
    kind: "core:directory",
    parent: null,
    ancestors: [],
    label: "Directory",
    origin: { kind: "builtin", id: "core.directory" },
    actions: [],
  },
  {
    kind: "azvs:game",
    parent: "core:directory",
    ancestors: ["core:directory"],
    label: "Game Library",
    origin: { kind: "plugin", id: "azvs.game" },
    actions: [],
  },
];

describe("Host Directory navigation", () => {
  it("keeps breadcrumbs and kind editing in the Host-owned shell", () => {
    const onNavigate = vi.fn();
    const onKindChange = vi.fn();
    render(
      <>
        <DirectoryBreadcrumbs path="library/games" onNavigate={onNavigate} />
        <DirectoryKindEditor
          directory={{ ...directory(), parentId: "parent-1" }}
          kinds={kinds}
          pending={false}
          onKindChange={onKindChange}
        />
      </>,
    );

    fireEvent.click(screen.getByRole("button", { name: "library" }));
    expect(onNavigate).toHaveBeenCalledWith("library");

    fireEvent.change(screen.getByLabelText("Directory kind"), { target: { value: "azvs:game" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    expect(onKindChange).toHaveBeenCalledWith("azvs:game");
  });

  it("keeps the root Directory kind immutable", () => {
    render(
      <DirectoryKindEditor
        directory={{ ...directory(), id: "00000000-0000-0000-0000-000000000000", path: "" }}
        kinds={kinds}
        pending={false}
        onKindChange={vi.fn()}
      />,
    );

    expect(screen.getByLabelText("Directory kind")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
  });
});
