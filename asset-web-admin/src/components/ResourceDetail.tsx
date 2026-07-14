import React from "react";
import { BookOpen, Download, Eye, Loader2, RotateCcw, Save, Trash2 } from "lucide-react";
import { apiBase } from "../api";
import type { Draft, Resource, ResourceActionDefinition, ResourceKindOption, ResourceStatus } from "../types";
import { formatBytes, formatDate, hasAction, isPluginUiAction } from "../utils/resourceDrafts";
import { Fact, SelectInput, TextInput } from "./forms";
import { cx, dangerIconButtonClass, iconButtonClass, inputClass, primaryButtonClass, secondaryButtonClass } from "./ui";

export function ResourceDetail({
  resource,
  draft,
  setDraft,
  resourceKinds,
  busy,
  onSave,
  onRead,
  onPreview,
  onPluginAction,
  onDelete,
  onRestore,
}: {
  resource: Resource;
  draft: Draft;
  setDraft: React.Dispatch<React.SetStateAction<Draft | null>>;
  resourceKinds: ResourceKindOption[];
  busy: boolean;
  onSave: () => void;
  onRead: () => void;
  onPreview: () => void;
  onPluginAction: (action: ResourceActionDefinition) => void;
  onDelete: () => void;
  onRestore: () => void;
}) {
  const kindDefinition = resourceKinds.find((kind) => kind.kind === resource.kind);
  const canRead = hasAction(resource, "read");
  const canPreview = hasAction(resource, "preview") || hasAction(resource, "view_inline");
  const pluginActions = resource.actions.available_actions
    .filter((action) => isPluginUiAction(action) && action.ui.locations.includes("resource_detail"))
    .sort((left, right) =>
      (left.ui.group ?? "").localeCompare(right.ui.group ?? "")
      || (left.ui.order ?? 0) - (right.ui.order ?? 0)
      || left.label.localeCompare(right.label));

  return (
    <div className="flex flex-col gap-6 p-6 max-sm:p-4">
      <header className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h2 className="break-words text-xl font-bold text-slate-900">{resource.name}</h2>
          <span className="mt-1 block truncate font-mono text-xs text-slate-400">{resource.id}</span>
        </div>
        <span className={cx("inline-flex rounded-full px-2.5 py-1 text-xs font-semibold", resource.status === "active" ? "bg-emerald-100 text-emerald-700" : "bg-slate-200 text-slate-600")}>{resource.status}</span>
      </header>

      <div className="flex flex-wrap items-center gap-2">
        <button className={primaryButtonClass} type="button" onClick={onSave} disabled={busy || Boolean(resource.deleted_at)}>
          {busy ? <Loader2 className="animate-spin" size={18} /> : <Save size={18} />}
          Save
        </button>
        {hasAction(resource, "download_content") && (
          <a className={iconButtonClass} href={`${apiBase}/resources/${resource.id}/content`} title="Download">
            <Download size={18} />
          </a>
        )}
        {canRead && (
          <button className={iconButtonClass} type="button" onClick={onRead} disabled={busy} title="Read">
            <BookOpen size={18} />
          </button>
        )}
        {canPreview && (
          <button className={iconButtonClass} type="button" onClick={onPreview} disabled={busy} title="Preview">
            <Eye size={18} />
          </button>
        )}
        {pluginActions.map((action) => (
          <button
            key={action.id}
            className={secondaryButtonClass}
            type="button"
            onClick={() => onPluginAction(action)}
            disabled={busy}
            title={`${action.label} (${action.access})`}
          >
            {action.label}
          </button>
        ))}
        {resource.deleted_at ? (
          <button className={iconButtonClass} type="button" onClick={onRestore} disabled={busy} title="Restore">
            <RotateCcw size={18} />
          </button>
        ) : (
          <button className={dangerIconButtonClass} type="button" onClick={onDelete} disabled={busy} title="Delete">
            <Trash2 size={18} />
          </button>
        )}
      </div>

      <div className="grid grid-cols-2 gap-4 max-sm:grid-cols-1">
        <TextInput label="Name" value={draft.name} onChange={(name) => setDraft((d) => d && { ...d, name })} />
        <TextInput
          label="Directory"
          value={draft.directory}
          onChange={(directory) => setDraft((d) => d && { ...d, directory })}
        />
        <SelectInput
          label="Kind"
          value={draft.kind}
          options={resourceKinds}
          onChange={(kind) => setDraft((d) => d && { ...d, kind })}
        />
        <label className="grid gap-2">
          <span className="text-xs font-semibold text-slate-600">Status</span>
          <select
            className={inputClass}
            value={draft.status}
            onChange={(event) => setDraft((d) => d && { ...d, status: event.target.value as ResourceStatus })}
            disabled={Boolean(resource.deleted_at)}
          >
            <option value="active">active</option>
            <option value="archived">archived</option>
          </select>
        </label>
        <TextInput
          label="Description"
          value={draft.description}
          onChange={(description) => setDraft((d) => d && { ...d, description })}
        />
        <TextInput label="Tags" value={draft.tags} onChange={(tags) => setDraft((d) => d && { ...d, tags })} />
      </div>

      <section className="grid grid-cols-2 gap-x-4 max-sm:grid-cols-1">
        <Fact label="Created" value={formatDate(resource.created_at)} />
        <Fact label="Updated" value={formatDate(resource.updated_at)} />
        <Fact label="Deleted" value={resource.deleted_at ? formatDate(resource.deleted_at) : "-"} />
        <Fact label="Directory" value={resource.directory || "/"} />
        <Fact label="Object" value={resource.content?.key ?? "-"} />
        <Fact label="Size" value={formatBytes(resource.content?.size ?? 0)} />
        <Fact label="MIME" value={resource.content?.mime_type ?? "-"} />
        <Fact label="Kind source" value={kindDefinition?.source ?? "-"} />
        <Fact label="Content kind" value={kindDefinition ? (kindDefinition.supports_content ? "yes" : "no") : "-"} />
        <Fact label="Kind actions" value={kindDefinition?.actions.map((action) => action.id).join(", ") || "-"} />
        <Fact
          label="Available actions"
          value={resource.actions.available_actions.map((action) => action.id).join(", ") || "-"}
        />
      </section>
    </div>
  );
}
