import React from "react";
import { request } from "../../api";
import type { Draft, PluginActionOutput, Resource, ResourceActionDefinition, ResourceDirectory, ResourceReadResponse, ScanStorageResponse, UploadDraft } from "../../types";
import { errorMessage, metadataFromDraft, metadataFromUpload, normalizeDirectoryInput, toDraft } from "../../utils/resourceDrafts";

type Dependencies = {
  currentDirectory: string;
  reload: () => Promise<void>;
  setError: React.Dispatch<React.SetStateAction<string | null>>;
};

export function useResourceMutations({ currentDirectory, reload, setError }: Dependencies) {
  const [selected, setSelected] = React.useState<Resource | null>(null);
  const [draft, setDraft] = React.useState<Draft | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [notice, setNotice] = React.useState<string | null>(null);
  const [reader, setReader] = React.useState<ResourceReadResponse | null>(null);
  const [previewResource, setPreviewResource] = React.useState<Resource | null>(null);
  const [pluginOutput, setPluginOutput] = React.useState<PluginActionOutput | null>(null);

  function select(resource: Resource | null) {
    setSelected(resource);
    setDraft(resource ? toDraft(resource) : null);
  }

  async function perform<T>(operation: () => Promise<T>, message?: string): Promise<T | undefined> {
    setBusy(true); setError(null);
    try {
      const result = await operation();
      if (message) setNotice(message);
      return result;
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }

  async function create(draft: Draft) {
    const created = await perform(() => request<Resource>("/resources", {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: draft.name, directory: normalizeDirectoryInput(draft.directory), kind: draft.kind,
        status: draft.status, metadata: metadataFromDraft(draft) }),
    }), "Created");
    if (created) { select(created); await reload(); }
    return created;
  }

  async function createFolder(name: string) {
    const created = await perform(() => request<ResourceDirectory>("/directories", {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ parent_path: currentDirectory, name }),
    }), "Folder created");
    if (created) await reload();
    return created;
  }

  async function save() {
    if (!selected || !draft) return;
    const updated = await perform(() => request<Resource>(`/resources/${selected.id}`, {
      method: "PATCH", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: draft.name, directory: normalizeDirectoryInput(draft.directory), kind: draft.kind,
        status: draft.status, metadata: metadataFromDraft(draft) }),
    }), "Saved");
    if (updated) { select(updated); await reload(); }
  }

  async function remove() {
    if (!selected) return;
    const deleted = await perform(() => request<Resource>(`/resources/${selected.id}`, { method: "DELETE" }), "Deleted");
    if (deleted) { select(deleted); await reload(); }
  }

  async function restore() {
    if (!selected) return;
    const restored = await perform(() => request<Resource>(`/resources/${selected.id}`, {
      method: "PATCH", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ restore: true, status: "active" }),
    }), "Restored");
    if (restored) { select(restored); await reload(); }
  }

  async function read() {
    if (!selected) return;
    const result = await perform(() => request<ResourceReadResponse>(`/resources/${selected.id}/read`));
    if (result) setReader(result);
  }

  async function runAction(action: ResourceActionDefinition) {
    if (!selected) return;
    const result = await perform(() => request<PluginActionOutput>(
      `/resources/${selected.id}/actions/${encodeURIComponent(action.id)}`,
      { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ input: {} }) },
    ));
    if (result) setPluginOutput(result);
  }

  async function upload(draft: UploadDraft) {
    if (!draft.file) { setError("Select a file first"); return; }
    const file = draft.file;
    const params = new URLSearchParams({ name: draft.name.trim() || file.name,
      directory: normalizeDirectoryInput(draft.directory), metadata_json: JSON.stringify(metadataFromUpload(draft)),
      original_filename: file.name });
    if (draft.kind.trim()) params.set("kind", draft.kind.trim());
    const uploaded = await perform(() => request<Resource>(`/resources/content/stream?${params}`, {
      method: "PUT", headers: { "Content-Type": file.type || "application/octet-stream" }, body: file,
    }), "Uploaded");
    if (uploaded) { select(uploaded); await reload(); }
    return uploaded;
  }

  async function scan() {
    setNotice(null);
    const result = await perform(() => request<ScanStorageResponse>("/scan", {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path: currentDirectory, sha256: true }),
    }));
    if (result) {
      setNotice(`Scanned ${result.scanned}, imported ${result.imported}, skipped ${result.skipped}`);
      await reload();
    }
  }

  return { selected, draft, setDraft, select, busy, notice, setNotice, reader, setReader,
    previewResource, setPreviewResource, pluginOutput, setPluginOutput, create, createFolder,
    save, remove, restore, read, runAction, upload, scan };
}
