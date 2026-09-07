import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { Resource } from "@/domain/resource";
import { ResourceDetail } from "./resource-detail";

vi.mock("@/shared/api/gateway-context", () => ({ useAssetWorkspaceGateway: () => ({}) }));

it("preserves a dirty draft across background revisions and requires an explicit reload before saving the new snapshot", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const client = new QueryClient();
  const onSave = vi.fn().mockResolvedValue(undefined);
  const resource: Resource = {
    id: "r",
    name: "original",
    directoryId: "d",
    directory: "",
    revision: 1,
    content: null,
    state: { lifecycle: { status: "active" }, content: "absent", effective: "no_content" },
    createdAt: "2026-09-07",
    updatedAt: "2026-09-07",
  };
  const render = (value: Resource) =>
    root.render(
      <QueryClientProvider client={client}>
        <ResourceDetail
          resource={value}
          selected
          loading={false}
          pending={false}
          error={null}
          onRetry={() => {}}
          onSave={onSave}
        />
      </QueryClientProvider>,
    );
  const button = (text: string) => {
    const element = Array.from(host.querySelectorAll("button")).find(
      (item) => item.textContent === text,
    );
    if (!element) throw new Error(`Missing button: ${text}`);
    return element;
  };
  const editName = (name: string) => {
    const input = host.querySelector<HTMLInputElement>('input[name="name"]');
    if (!input) throw new Error("Missing name input");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(input, name);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  };
  try {
    await act(() => render(resource));
    await act(() => button("Edit").click());
    await act(() => editName("my draft"));
    const latest = { ...resource, revision: 2, name: "server change" };
    await act(() => render(latest));
    expect(host.querySelector<HTMLInputElement>('input[name="name"]')?.value).toBe("my draft");
    expect(button("Save").disabled).toBe(true);
    await act(() => button("Reload latest").click());
    expect(host.querySelector<HTMLInputElement>('input[name="name"]')?.value).toBe("server change");
    await act(() => editName("reviewed draft"));
    await act(() => button("Save").click());
    expect(onSave).toHaveBeenCalledWith(latest, { name: "reviewed draft", directory: "" });
  } finally {
    await act(() => root.unmount());
    client.clear();
    host.remove();
    vi.unstubAllGlobals();
  }
});
