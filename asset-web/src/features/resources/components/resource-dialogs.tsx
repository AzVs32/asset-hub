import { zodResolver } from "@hookform/resolvers/zod";
import { FileUp, FolderPlus, Plus } from "lucide-react";
import React from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import type { ResourceDraft, ResourceKind, UploadDraft } from "@/domain/resource";
import { emptyResourceDraft } from "@/domain/resource-draft";
import { Button } from "@/shared/ui/button";
import { Dialog } from "@/shared/ui/dialog";
import { controlClass, Field, Input, Textarea } from "@/shared/ui/field";

const resourceSchema = z.object({
  name: z.string().refine((value) => value.trim().length > 0, "Name is required"),
  directory: z.string(),
  kind: z.string().trim().min(1, "Kind is required"),
  status: z.enum(["active", "archived"]),
  description: z.string(),
  tags: z.string(),
});

export function CreateResourceDialog({
  open,
  onOpenChange,
  directory,
  kinds,
  pending,
  onCreate,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  directory: string;
  kinds: ResourceKind[];
  pending: boolean;
  onCreate: (draft: ResourceDraft) => Promise<unknown>;
}) {
  const form = useForm<ResourceDraft>({
    resolver: zodResolver(resourceSchema),
    defaultValues: emptyResourceDraft(directory, kinds),
  });
  React.useEffect(() => {
    if (open) form.reset(emptyResourceDraft(directory, kinds));
  }, [directory, form, kinds, open]);

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="New resource"
      description="Create a resource without uploading content."
    >
      <form
        className="grid gap-4 p-6 sm:grid-cols-2"
        onSubmit={form.handleSubmit(async (draft) => {
          await onCreate(draft);
          onOpenChange(false);
        })}
      >
        <Field label="Name" error={form.formState.errors.name?.message}>
          <Input {...form.register("name")} />
        </Field>
        <Field label="Directory">
          <Input {...form.register("directory")} />
        </Field>
        <Field label="Kind" error={form.formState.errors.kind?.message}>
          <select className={controlClass} {...form.register("kind")}>
            {kinds.map((kind) => (
              <option key={kind.kind} value={kind.kind}>
                {kind.label} · {kind.kind}
              </option>
            ))}
          </select>
        </Field>
        <Field label="Status">
          <select className={controlClass} {...form.register("status")}>
            <option value="active">Active</option>
            <option value="archived">Archived</option>
          </select>
        </Field>
        <div className="sm:col-span-2">
          <Field label="Description">
            <Textarea {...form.register("description")} />
          </Field>
        </div>
        <div className="sm:col-span-2">
          <Field label="Tags" error={form.formState.errors.tags?.message}>
            <Input placeholder="reference, approved, 2026" {...form.register("tags")} />
          </Field>
        </div>
        <div className="flex justify-end gap-2 border-t border-slate-100 pt-4 sm:col-span-2">
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button type="submit" disabled={pending}>
            <Plus size={17} />
            {pending ? "Creating…" : "Create"}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

interface UploadForm {
  file: FileList;
  name: string;
  directory: string;
  kind: string;
  description: string;
  tags: string;
}

export function UploadResourceDialog({
  open,
  onOpenChange,
  directory,
  kinds,
  pending,
  onUpload,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  directory: string;
  kinds: ResourceKind[];
  pending: boolean;
  onUpload: (draft: UploadDraft) => Promise<unknown>;
}) {
  const form = useForm<UploadForm>({
    defaultValues: { name: "", directory, kind: "", description: "", tags: "" },
  });
  React.useEffect(() => {
    if (open) form.reset({ name: "", directory, kind: "", description: "", tags: "" });
  }, [directory, form, open]);
  const file = form.watch("file")?.item(0);

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="Upload asset"
      description="The server can detect the kind from file content and extension."
    >
      <form
        className="grid gap-4 p-6 sm:grid-cols-2"
        onSubmit={form.handleSubmit(async (input) => {
          const selected = input.file.item(0);
          if (!selected) return;
          await onUpload({
            file: selected,
            name: input.name,
            directory: input.directory,
            kind: input.kind,
            description: input.description,
            tags: input.tags,
          });
          onOpenChange(false);
        })}
      >
        <label className="sm:col-span-2 flex min-h-28 cursor-pointer items-center justify-center gap-3 rounded-2xl border-2 border-dashed border-slate-300 bg-slate-50 p-5 text-sm font-semibold text-slate-600 hover:border-blue-400 hover:bg-blue-50">
          <FileUp size={22} />
          <span>{file?.name ?? "Choose a file"}</span>
          <input className="sr-only" type="file" {...form.register("file", { required: true })} />
        </label>
        <Field label="Display name">
          <Input placeholder={file?.name ?? "Defaults to filename"} {...form.register("name")} />
        </Field>
        <Field label="Directory">
          <Input {...form.register("directory")} />
        </Field>
        <Field label="Kind">
          <select className={controlClass} {...form.register("kind")}>
            <option value="">Automatic detection</option>
            {kinds
              .filter((kind) => kind.supportsContent)
              .map((kind) => (
                <option key={kind.kind} value={kind.kind}>
                  {kind.label} · {kind.kind}
                </option>
              ))}
          </select>
        </Field>
        <Field label="Tags">
          <Input {...form.register("tags")} />
        </Field>
        <div className="sm:col-span-2">
          <Field label="Description">
            <Textarea {...form.register("description")} />
          </Field>
        </div>
        <div className="flex justify-end gap-2 border-t border-slate-100 pt-4 sm:col-span-2">
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button type="submit" disabled={pending || !file}>
            <FileUp size={17} />
            {pending ? "Uploading…" : "Upload"}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

export function CreateFolderDialog({
  open,
  onOpenChange,
  parent,
  pending,
  onCreate,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  parent: string;
  pending: boolean;
  onCreate: (name: string) => Promise<unknown>;
}) {
  const form = useForm<{ name: string }>({ defaultValues: { name: "" } });
  React.useEffect(() => {
    if (open) form.reset();
  }, [form, open]);
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="New folder"
      description={`Inside /${parent}`}
    >
      <form
        className="grid gap-5 p-6"
        onSubmit={form.handleSubmit(async ({ name }) => {
          await onCreate(name);
          onOpenChange(false);
        })}
      >
        <Field label="Folder name" error={form.formState.errors.name?.message}>
          <Input
            autoFocus
            {...form.register("name", {
              validate: (value) => value.trim().length > 0 || "Folder name is required",
            })}
          />
        </Field>
        <div className="flex justify-end gap-2">
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button type="submit" disabled={pending}>
            <FolderPlus size={17} />
            {pending ? "Creating…" : "Create"}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
