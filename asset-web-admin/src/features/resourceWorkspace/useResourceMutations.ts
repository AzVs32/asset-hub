import React from "react";
import { request } from "../../api";
import type { PluginActionOutput, Resource, ResourceActionDefinition, ResourceDirectory, ScanStorageResponse } from "../../api/contracts";
import { executeResourceAction } from "../../plugins/host/actions";
import { errorMessage, metadataFromDraft, metadataFromUpload, normalizeDirectoryInput, toDraft } from "../../utils/resourceDrafts";
import type { Draft, UploadDraft } from "./models";

export type ActionResult = {
  resource: Resource;
  output: PluginActionOutput;
};

type Dependencies = {
  currentDirectory: string;
  reload: () => Promise<void>;
  setError: React.Dispatch<React.SetStateAction<string | null>>;
};

export function useResourceMutations({ currentDirectory, reload, setError }: Dependencies) {
  const [selected, setSelected] = React.useState<Resource | null>(null);
  const [draft, setDraft] = React.useState<Draft | null>(null);
  const [pendingOperations, setPendingOperations] = React.useState<Set<string>>(() => new Set());
  const pendingRef = React.useRef(new Set<string>());
  const [notice, setNotice] = React.useState<string | null>(null);
  const [actionResult, setActionResult] = React.useState<ActionResult | null>(null);

  function select(resource: Resource | null) {
    setSelected(resource);
    setDraft(resource ? toDraft(resource) : null);
  }

  async function perform<T>(key: string, operation: () => Promise<T>, message?: string): Promise<T | undefined> {
    if (pendingRef.current.has(key)) return undefined;
    pendingRef.current.add(key);
    setPendingOperations(new Set(pendingRef.current)); setError(null);
    try {
      const result = await operation();
      if (message) setNotice(message);
      return result;
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      pendingRef.current.delete(key);
      setPendingOperations(new Set(pendingRef.current));
    }
  }

  async function create(draft: Draft) {
    const created = await perform("create", () => request<Resource>("/resources", {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: draft.name, directory: normalizeDirectoryInput(draft.directory), kind: draft.kind,
        status: draft.status, metadata: metadataFromDraft(draft) }),
    }), "Created");
    if (created) { select(created); await reload(); }
    return created;
  }

  async function createFolder(name: string) {
    const created = await perform("create-folder", () => request<ResourceDirectory>("/directories", {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ parent_path: currentDirectory, name }),
    }), "Folder created");
    if (created) await reload();
    return created;
  }

  async function save() {
    if (!selected || !draft) return;
    const updated = await perform(`save:${selected.id}`, () => request<Resource>(`/resources/${selected.id}`, {
      method: "PATCH", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: draft.name, directory: normalizeDirectoryInput(draft.directory), kind: draft.kind,
        status: draft.status, metadata: metadataFromDraft(draft) }),
    }), "Saved");
    if (updated) { select(updated); await reload(); }
  }

  async function remove() {
    if (!selected) return;
    const deleted = await perform(`delete:${selected.id}`, () => request<Resource>(`/resources/${selected.id}`, { method: "DELETE" }), "Deleted");
    if (deleted) { select(deleted); await reload(); }
  }

  async function restore() {
    if (!selected) return;
    const restored = await perform(`restore:${selected.id}`, () => request<Resource>(`/resources/${selected.id}`, {
      method: "PATCH", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ restore: true, status: "active" }),
    }), "Restored");
    if (restored) { select(restored); await reload(); }
  }

  async function refreshResource(resourceId: string) {
    const refreshed = await request<Resource>(`/resources/${encodeURIComponent(resourceId)}`);
    if (selected?.id === resourceId) select(refreshed);
    setActionResult((current) => current?.resource.id === resourceId
      ? { ...current, resource: refreshed }
      : current);
    await reload();
  }

  async function runAction(resource: Resource, action: ResourceActionDefinition) {
    const result = await perform(
      `action:${resource.id}:${action.id}`,
      () => executeResourceAction(resource, action.id),
    );
    if (!result) return;
    setActionResult({ resource, output: result });
    if (action.access === "read_write") await refreshResource(resource.id);
  }

  async function upload(draft: UploadDraft) {
    if (!draft.file) { setError("Select a file first"); return; }
    const file = draft.file;
    const params = new URLSearchParams({ name: draft.name.trim() || file.name,
      directory: normalizeDirectoryInput(draft.directory), metadata_json: JSON.stringify(metadataFromUpload(draft)),
      original_filename: file.name });
    if (draft.kind.trim()) params.set("kind", draft.kind.trim());
    const uploaded = await perform("upload", () => request<Resource>(`/resources/content/stream?${params}`, {
      method: "PUT", headers: { "Content-Type": file.type || "application/octet-stream" }, body: file,
    }), "Uploaded");
    if (uploaded) { select(uploaded); await reload(); }
    return uploaded;
  }

  async function scan() {
    setNotice(null);
    const result = await perform("scan", () => request<ScanStorageResponse>("/scan", {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ directory: currentDirectory, sha256: true }),
    }));
    if (result) {
      setNotice(`Scanned ${result.scanned}, imported ${result.imported}, skipped ${result.skipped}`);
      await reload();
    }
  }

  const isPending = (key: string) => pendingOperations.has(key);
  return { selected, draft, setDraft, select, pendingOperations, isPending, notice, setNotice,
    actionResult, setActionResult, create, createFolder, save, remove, restore, runAction,
    refreshResource, upload, scan };
}
