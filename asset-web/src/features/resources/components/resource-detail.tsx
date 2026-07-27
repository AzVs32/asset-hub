import { zodResolver } from "@hookform/resolvers/zod";
import {
  Database,
  Download,
  Eye,
  FileJson,
  Pencil,
  Play,
  RotateCcw,
  Save,
  Trash2,
} from "lucide-react";
import React from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import type { Resource, ResourceAction, ResourceDraft, ResourceKind } from "@/domain/resource";
import { draftFromResource, formatBytes, formatDate } from "@/domain/resource-draft";
import { usePluginKernel } from "@/kernel/plugin-kernel";
import { hostSlots } from "@/kernel/slots";
import { AutomaticSlot } from "@/plugins/automatic-slot";
import { Button } from "@/shared/ui/button";
import { controlClass, Field, Input } from "@/shared/ui/field";

const draftSchema = z.object({
  name: z.string().refine((value) => value.trim().length > 0, "Name is required"),
  directory: z.string(),
  kind: z.string().trim().min(1),
  tags: z.string(),
});

export function ResourceDetail({
  resource,
  kinds,
  pending,
  onSave,
  onAction,
  onDelete,
  onRestore,
  onResourceChanged,
}: {
  resource: Resource | null;
  kinds: ResourceKind[];
  pending: boolean;
  onSave: (draft: ResourceDraft) => Promise<unknown>;
  onAction: (action: ResourceAction) => void;
  onDelete: () => void;
  onRestore: () => void;
  onResourceChanged: () => void | Promise<void>;
}) {
  if (!resource) {
    return (
      <aside className="grid min-h-72 place-items-center border-l border-slate-200 bg-slate-50/60 text-slate-400">
        <span className="grid justify-items-center gap-3 text-sm">
          <Database size={30} />
          Select a resource or directory to inspect it
        </span>
      </aside>
    );
  }
  return (
    <Detail
      resource={resource}
      kinds={kinds}
      pending={pending}
      onSave={onSave}
      onAction={onAction}
      onDelete={onDelete}
      onRestore={onRestore}
      onResourceChanged={onResourceChanged}
    />
  );
}

function Detail({
  resource,
  kinds,
  pending,
  onSave,
  onAction,
  onDelete,
  onRestore,
  onResourceChanged,
}: {
  resource: Resource;
  kinds: ResourceKind[];
  pending: boolean;
  onSave: (draft: ResourceDraft) => Promise<unknown>;
  onAction: (action: ResourceAction) => void;
  onDelete: () => void;
  onRestore: () => void;
  onResourceChanged: () => void | Promise<void>;
}) {
  const kernel = usePluginKernel();
  const actions = kernel.actionsAt(resource, hostSlots.resourceDetailActions);
  const displayResource = {
    ...resource,
    directory: resource.directory || "/",
  };
  const form = useForm<ResourceDraft>({
    resolver: zodResolver(draftSchema),
    defaultValues: draftFromResource(displayResource),
  });
  React.useEffect(
    () => form.reset(draftFromResource({ ...resource, directory: displayResource.directory })),
    [displayResource.directory, form, resource],
  );
  const kind = kinds.find((candidate) => candidate.kind === resource.kind);

  return (
    <aside
      className="min-h-0 overflow-auto border-l border-slate-200 bg-slate-50/60"
      aria-label="Resource details"
    >
      <div className="grid gap-5 p-5 xl:p-6">
        <header className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <h2 className="break-words text-xl font-bold text-slate-950">{resource.name}</h2>
            <code className="mt-1 block truncate text-[11px] text-slate-400">{resource.id}</code>
          </div>
          {resource.deletedAt ? (
            <span className="shrink-0 rounded-full bg-red-100 px-2.5 py-1 text-xs font-semibold text-red-700">
              deleted
            </span>
          ) : null}
        </header>

        <div className="flex flex-wrap gap-2" data-plugin-slot={hostSlots.resourceDetailActions}>
          <Button
            size="small"
            disabled={pending || Boolean(resource.deletedAt) || !form.formState.isDirty}
            onClick={form.handleSubmit(onSave)}
          >
            <Save size={16} />
            Save
          </Button>
          {actions.map((action) => (
            <Button
              key={action.id}
              variant="secondary"
              size="small"
              disabled={pending}
              title={action.description ?? action.id}
              onClick={() => onAction(action)}
            >
              <ActionIcon action={action} />
              {action.label}
            </Button>
          ))}
          {resource.deletedAt ? (
            <Button
              variant="secondary"
              size="icon"
              aria-label="Restore resource"
              disabled={pending}
              onClick={onRestore}
            >
              <RotateCcw size={17} />
            </Button>
          ) : (
            <Button
              variant="ghost"
              size="icon"
              className="text-red-600"
              aria-label="Delete resource"
              disabled={pending}
              onClick={onDelete}
            >
              <Trash2 size={17} />
            </Button>
          )}
        </div>

        <AutomaticSlot
          slot={hostSlots.resourceDetailAside}
          resource={resource}
          onResourceChanged={onResourceChanged}
        />

        <form
          className="grid gap-4 rounded-2xl border border-slate-200 bg-white p-4 sm:grid-cols-2"
          onSubmit={form.handleSubmit(onSave)}
        >
          <Field label="Name" error={form.formState.errors.name?.message}>
            <Input disabled={Boolean(resource.deletedAt)} {...form.register("name")} />
          </Field>
          <Field label="Directory">
            <Input disabled={Boolean(resource.deletedAt)} {...form.register("directory")} />
          </Field>
          <Field label="Kind">
            <select
              className={controlClass}
              disabled={Boolean(resource.deletedAt)}
              {...form.register("kind")}
            >
              {kinds.map((item) => (
                <option key={item.kind} value={item.kind}>
                  {item.label}
                </option>
              ))}
            </select>
          </Field>
          <div className="sm:col-span-2">
            <Field label="Tags">
              <Input disabled={Boolean(resource.deletedAt)} {...form.register("tags")} />
            </Field>
          </div>
        </form>

        <section className="grid grid-cols-2 gap-x-4 rounded-2xl border border-slate-200 bg-white p-4 text-sm">
          <Fact label="Created" value={formatDate(resource.createdAt)} />
          <Fact label="Updated" value={formatDate(resource.updatedAt)} />
          <Fact label="Directory" value={displayResource.directory} />
          <Fact label="Size" value={formatBytes(resource.content?.size ?? 0)} />
          <Fact label="MIME" value={resource.content?.mimeType ?? "—"} />
          <Fact label="Kind source" value={kind?.source ?? "—"} />
          <Fact
            label="Content"
            value={
              resource.content
                ? resource.directory
                  ? `${resource.directory}/${resource.name}`
                  : `/${resource.name}`
                : "—"
            }
            wide
          />
          <Fact
            label="Actions"
            value={resource.actions.map((action) => action.id).join(", ") || "—"}
            wide
          />
        </section>

        <AutomaticSlot
          slot={hostSlots.resourceDetailPanel}
          resource={resource}
          onResourceChanged={onResourceChanged}
        />
      </div>
    </aside>
  );
}

function Fact({ label, value, wide = false }: { label: string; value: string; wide?: boolean }) {
  return (
    <div className={`min-w-0 border-b border-slate-100 py-2 ${wide ? "col-span-2" : ""}`}>
      <dt className="text-[11px] font-semibold uppercase tracking-wide text-slate-400">{label}</dt>
      <dd className="mt-1 break-words text-slate-700">{value}</dd>
    </div>
  );
}

function ActionIcon({ action }: { action: ResourceAction }) {
  if (action.access === "read_write") return <Pencil size={15} />;
  if (action.output.views.includes("download")) return <Download size={15} />;
  if (action.output.views.includes("media") || action.output.views.includes("plugin_frame"))
    return <Eye size={15} />;
  if (action.output.views.includes("json")) return <FileJson size={15} />;
  return <Play size={15} />;
}
