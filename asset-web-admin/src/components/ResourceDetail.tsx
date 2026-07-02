import React from "react";
import { BookOpen, Download, Eye, Loader2, RotateCcw, Save, Trash2 } from "lucide-react";
import { apiBase } from "../api";
import type { Draft, Resource, ResourceActionDefinition, ResourceKindOption, ResourceStatus } from "../types";
import { formatBytes, formatDate, isPluginUiAction, withKindDefaults } from "../utils/resourceDrafts";
import { Fact, SelectInput, TextInput } from "./forms";

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
  const canRead = resource.actions.read;
  const canPreview = resource.actions.preview || resource.actions.view_inline;
  const pluginActions = resource.actions.available_actions.filter(isPluginUiAction);

  return (
    <div className="detail-content">
      <header className="detail-header">
        <div>
          <h2>{resource.name}</h2>
          <span>{resource.id}</span>
        </div>
        <span className={`status-pill ${resource.status}`}>{resource.status}</span>
      </header>

      <div className="detail-actions">
        <button className="primary-button" type="button" onClick={onSave} disabled={busy || Boolean(resource.deleted_at)}>
          {busy ? <Loader2 className="spin" size={18} /> : <Save size={18} />}
          Save
        </button>
        {resource.actions.download_content && (
          <a className="icon-button" href={`${apiBase}/resources/${resource.id}/content`} title="Download">
            <Download size={18} />
          </a>
        )}
        {canRead && (
          <button className="icon-button" type="button" onClick={onRead} disabled={busy} title="Read">
            <BookOpen size={18} />
          </button>
        )}
        {canPreview && (
          <button className="icon-button" type="button" onClick={onPreview} disabled={busy} title="Preview">
            <Eye size={18} />
          </button>
        )}
        {pluginActions.map((action) => (
          <button
            key={action.id}
            className="ghost-button"
            type="button"
            onClick={() => onPluginAction(action)}
            disabled={busy}
            title={`${action.label} (${action.access})`}
          >
            {action.label}
          </button>
        ))}
        {resource.deleted_at ? (
          <button className="icon-button" type="button" onClick={onRestore} disabled={busy} title="Restore">
            <RotateCcw size={18} />
          </button>
        ) : (
          <button className="icon-button danger" type="button" onClick={onDelete} disabled={busy} title="Delete">
            <Trash2 size={18} />
          </button>
        )}
      </div>

      <div className="form-grid detail-form">
        <TextInput label="Name" value={draft.name} onChange={(name) => setDraft((d) => d && { ...d, name })} />
        <SelectInput
          label="Kind"
          value={draft.kind}
          options={resourceKinds}
          onChange={(kind) => setDraft((d) => d && withKindDefaults({ ...d, kind }, resourceKinds))}
        />
        <label className="field">
          <span>Status</span>
          <select
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
        <TextInput
          label="Schema ID"
          value={draft.schemaId}
          onChange={(schemaId) => setDraft((d) => d && { ...d, schemaId })}
        />
        <label className="field full">
          <span>Kind data JSON</span>
          <textarea
            value={draft.kindData}
            onChange={(event) => setDraft((d) => d && { ...d, kindData: event.target.value })}
            rows={7}
            disabled={Boolean(resource.deleted_at)}
          />
        </label>
      </div>

      <section className="facts">
        <Fact label="Created" value={formatDate(resource.created_at)} />
        <Fact label="Updated" value={formatDate(resource.updated_at)} />
        <Fact label="Deleted" value={resource.deleted_at ? formatDate(resource.deleted_at) : "-"} />
        <Fact label="Object" value={resource.content?.key ?? "-"} />
        <Fact label="Size" value={formatBytes(resource.content?.size ?? 0)} />
        <Fact label="MIME" value={resource.content?.mime_type ?? "-"} />
        <Fact label="Kind source" value={kindDefinition?.source ?? "-"} />
        <Fact label="Kind schema" value={kindDefinition?.schema_id ?? "-"} />
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



