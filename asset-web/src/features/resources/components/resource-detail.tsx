import { zodResolver } from "@hookform/resolvers/zod";
import { Database, FileText, Save } from "lucide-react";
import React from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import type { Resource, ResourceDraft, ResourceKind } from "@/domain/resource";
import { draftFromResource, formatBytes, formatDate } from "@/domain/resource-draft";
import { Button } from "@/shared/ui/button";
import { Field, Input } from "@/shared/ui/field";
import { KindSelect } from "./kind-select";

const draftSchema = z.object({
  name: z.string().refine((value) => value.trim().length > 0, "Name is required"),
  directory: z.string(),
  kind: z.string().trim().min(1),
});

interface ResourceDetailProps {
  resource: Resource | null;
  kinds: ResourceKind[];
  pending: boolean;
  onSave: (draft: ResourceDraft) => Promise<unknown>;
}

export function ResourceDetail({ resource, kinds, pending, onSave }: ResourceDetailProps) {
  if (!resource) {
    return (
      <aside className="grid min-h-72 place-items-center overflow-hidden rounded-3xl border border-slate-200/80 bg-white text-slate-400 shadow-[0_18px_50px_-30px_rgba(15,23,42,0.45)]">
        <span className="grid max-w-56 justify-items-center gap-3 text-center text-sm font-medium">
          <span className="grid size-14 place-items-center rounded-2xl bg-slate-100 text-slate-300">
            <Database size={25} />
          </span>
          Select an asset or folder to see its details
        </span>
      </aside>
    );
  }
  return <Detail resource={resource} kinds={kinds} pending={pending} onSave={onSave} />;
}

function Detail({
  resource,
  kinds,
  pending,
  onSave,
}: {
  resource: Resource;
  kinds: ResourceKind[];
  pending: boolean;
  onSave: (draft: ResourceDraft) => Promise<unknown>;
}) {
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
      className="min-h-0 overflow-auto rounded-3xl border border-slate-200/80 bg-white shadow-[0_18px_50px_-30px_rgba(15,23,42,0.45)]"
      aria-label="Resource details"
    >
      <div className="grid gap-5 p-5">
        <header className="flex items-start justify-between gap-4">
          <div className="flex min-w-0 items-start gap-3">
            <span className="grid size-11 shrink-0 place-items-center rounded-2xl bg-indigo-50 text-indigo-600 ring-1 ring-indigo-100">
              <FileText size={20} />
            </span>
            <div className="min-w-0 pt-0.5">
              <p className="text-[10px] font-bold uppercase tracking-[0.15em] text-slate-400">
                Asset details
              </p>
              <h2 className="mt-0.5 break-words text-lg font-bold tracking-[-0.025em] text-slate-950">
                {resource.name}
              </h2>
              <code className="mt-1 block truncate text-[10px] text-slate-400">{resource.id}</code>
            </div>
          </div>
          {resource.deletedAt ? (
            <span className="shrink-0 rounded-full bg-rose-50 px-2.5 py-1 text-xs font-semibold text-rose-700 ring-1 ring-rose-200">
              deleted
            </span>
          ) : null}
        </header>

        <form
          className="grid gap-3 rounded-2xl border border-slate-200/80 bg-slate-50/65 p-4"
          onSubmit={form.handleSubmit(onSave)}
        >
          <Field label="Name" error={form.formState.errors.name?.message}>
            <Input disabled={Boolean(resource.deletedAt)} {...form.register("name")} />
          </Field>
          <Field label="Directory">
            <Input disabled={Boolean(resource.deletedAt)} {...form.register("directory")} />
          </Field>
          <Field label="Kind">
            <KindSelect
              kinds={kinds}
              disabled={Boolean(resource.deletedAt)}
              {...form.register("kind")}
            />
          </Field>
          <div className="flex justify-end border-t border-slate-200/70 pt-3">
            <Button
              type="submit"
              size="small"
              disabled={pending || Boolean(resource.deletedAt) || !form.formState.isDirty}
            >
              <Save size={16} />
              Save
            </Button>
          </div>
        </form>

        <section className="grid grid-cols-2 gap-x-4 rounded-2xl border border-slate-200/80 bg-white p-4 text-sm shadow-sm">
          <Fact label="Created" value={formatDate(resource.createdAt)} />
          <Fact label="Updated" value={formatDate(resource.updatedAt)} />
          <Fact label="Directory" value={displayResource.directory} />
          <Fact label="Size" value={formatBytes(resource.content?.size ?? 0)} />
          <Fact label="MIME" value={resource.content?.mimeType ?? "—"} />
          <Fact
            label="Verification"
            value={
              resource.content
                ? resource.content.verificationStatus === "failed"
                  ? `failed: ${resource.content.verificationError ?? "unknown error"}`
                  : resource.content.verificationStatus
                : "—"
            }
          />
          <Fact label="Kind origin" value={kind ? `${kind.origin.kind}:${kind.origin.id}` : "—"} />
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
      </div>
    </aside>
  );
}

function Fact({ label, value, wide = false }: { label: string; value: string; wide?: boolean }) {
  return (
    <div className={`min-w-0 border-b border-slate-100 py-2.5 ${wide ? "col-span-2" : ""}`}>
      <dt className="text-[10px] font-bold uppercase tracking-[0.12em] text-slate-400">{label}</dt>
      <dd className="mt-1 break-words text-xs font-medium leading-5 text-slate-700">{value}</dd>
    </div>
  );
}
