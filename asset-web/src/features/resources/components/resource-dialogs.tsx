import { FileUp, FolderPlus, LoaderCircle } from "lucide-react";
import React from "react";
import { useForm } from "react-hook-form";
import type { ResourceKind, UploadDraft, UploadProgress } from "@/domain/resource";
import { Button } from "@/shared/ui/button";
import { Dialog } from "@/shared/ui/dialog";
import { Field, Input } from "@/shared/ui/field";
import { KindSelect } from "./kind-select";

interface UploadForm {
  file: FileList;
  name: string;
  directory: string;
  kind: string;
  tags: string;
}

export function UploadResourceDialog({
  open,
  onOpenChange,
  directory,
  kinds,
  pending,
  progress,
  onUpload,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  directory: string;
  kinds: ResourceKind[];
  pending: boolean;
  progress: UploadProgress | null;
  onUpload: (draft: UploadDraft) => Promise<unknown>;
}) {
  const form = useForm<UploadForm>({
    defaultValues: { name: "", directory, kind: "", tags: "" },
  });
  React.useEffect(() => {
    if (open) form.reset({ name: "", directory, kind: "", tags: "" });
  }, [directory, form, open]);
  const file = form.watch("file")?.item(0);

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!pending) onOpenChange(nextOpen);
      }}
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
          <KindSelect
            kinds={kinds}
            emptyOption={{ label: "Automatic detection" }}
            showKind
            isKindDisabled={(kind) => !kinds.find((item) => item.kind === kind)?.supportsContent}
            {...form.register("kind")}
          />
        </Field>
        <Field label="Tags">
          <Input {...form.register("tags")} />
        </Field>
        {pending && progress ? <UploadProgressView progress={progress} /> : null}
        <div className="flex justify-end gap-2 border-t border-slate-100 pt-4 sm:col-span-2">
          <Button variant="secondary" disabled={pending} onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button type="submit" disabled={pending || !file}>
            {pending ? <LoaderCircle className="animate-spin" size={17} /> : <FileUp size={17} />}
            {pending && progress ? uploadButtonLabel(progress) : "Upload"}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function UploadProgressView({ progress }: { progress: UploadProgress }) {
  const percentage =
    progress.totalBytes === 0
      ? progress.stage === "finalizing"
        ? 100
        : 0
      : Math.min(100, Math.floor((progress.bytesSent / progress.totalBytes) * 100));
  const label = {
    preparing: "Preparing upload…",
    uploading: "Uploading file…",
    finalizing: "Verifying and publishing resource…",
  }[progress.stage];
  const finalizing = progress.stage === "finalizing";

  return (
    <div
      className="grid gap-2 rounded-xl border border-blue-200 bg-blue-50 p-4 sm:col-span-2"
      aria-live="polite"
    >
      <div className="flex items-center justify-between gap-3 text-sm font-semibold text-slate-800">
        <span>{label}</span>
        <span>{finalizing ? "File uploaded" : `${percentage}%`}</span>
      </div>
      <div
        className="h-2.5 overflow-hidden rounded-full bg-blue-100"
        role="progressbar"
        aria-label={finalizing ? "Resource publishing progress" : "Upload progress"}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={finalizing ? undefined : percentage}
      >
        <div
          className={`h-full rounded-full bg-blue-600 transition-[width] duration-300 ${
            finalizing ? "animate-pulse" : ""
          }`}
          style={{ width: `${percentage}%` }}
        />
      </div>
      <p className="text-xs text-slate-600">
        {formatBytes(progress.bytesSent)} / {formatBytes(progress.totalBytes)}
        {progress.stage === "uploading"
          ? " · updates after each 8 MiB chunk"
          : finalizing
            ? " transferred · large files take longer to verify"
            : ""}
      </p>
    </div>
  );
}

function uploadButtonLabel(progress: UploadProgress): string {
  if (progress.stage === "preparing") return "Preparing…";
  if (progress.stage === "finalizing") return "Publishing…";
  const percentage =
    progress.totalBytes === 0
      ? 0
      : Math.min(100, Math.floor((progress.bytesSent / progress.totalBytes) * 100));
  return `Uploading ${percentage}%…`;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let value = bytes / 1024;
  let unit = units[0];
  for (let index = 1; index < units.length && value >= 1024; index += 1) {
    value /= 1024;
    unit = units[index];
  }
  return `${value >= 10 ? value.toFixed(1) : value.toFixed(2)} ${unit}`;
}

export function CreateFolderDialog({
  open,
  onOpenChange,
  parent,
  kinds,
  pending,
  onCreate,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  parent: string;
  kinds: import("@/domain/resource").DirectoryKind[];
  pending: boolean;
  onCreate: (name: string, kind?: string) => Promise<unknown>;
}) {
  const form = useForm<{ name: string; kind: string }>({
    defaultValues: { name: "", kind: "core:directory" },
  });
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
        onSubmit={form.handleSubmit(async ({ name, kind }) => {
          await onCreate(name, kind || undefined);
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
        <Field label="Folder kind">
          <KindSelect kinds={kinds} {...form.register("kind")} />
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
