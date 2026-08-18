import CreateNewFolderIcon from "@mui/icons-material/CreateNewFolder";
import UploadFileIcon from "@mui/icons-material/UploadFile";
import {
  Box,
  Button,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  LinearProgress,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import React from "react";
import { Controller, useForm } from "react-hook-form";
import type { ResourceKind, UploadDraft, UploadProgress } from "@/domain/resource";
import { KindSelect } from "./kind-select";

interface UploadForm {
  file: FileList;
  name: string;
  directory: string;
  kind: string;
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
    defaultValues: { name: "", directory, kind: "" },
  });
  React.useEffect(() => {
    if (open) form.reset({ name: "", directory, kind: "" });
  }, [directory, form, open]);
  const file = form.watch("file")?.item(0);

  return (
    <Dialog
      open={open}
      fullWidth
      maxWidth="sm"
      onClose={() => {
        if (!pending) onOpenChange(false);
      }}
    >
      <DialogTitle>Upload asset</DialogTitle>
      <DialogContent>
        <Box
          component="form"
          sx={{ display: "grid", gap: 2, pt: 1 }}
          onSubmit={form.handleSubmit(async (input) => {
            const selected = input.file.item(0);
            if (!selected) return;
            await onUpload({
              file: selected,
              name: input.name,
              directory: input.directory,
              kind: input.kind,
            });
            onOpenChange(false);
          })}
        >
          <Button
            component="label"
            variant="outlined"
            startIcon={<UploadFileIcon />}
            sx={{ minHeight: 128 }}
          >
            {file?.name ?? "Choose a file"}
            <input type="file" hidden {...form.register("file", { required: true })} />
          </Button>
          <Controller
            name="name"
            control={form.control}
            render={({ field }) => {
              const { ref, ...rest } = field;
              return (
                <TextField
                  {...rest}
                  inputRef={ref}
                  label="Display name"
                  placeholder={file?.name ?? "Defaults to filename"}
                />
              );
            }}
          />
          <Controller
            name="directory"
            control={form.control}
            render={({ field }) => {
              const { ref, ...rest } = field;
              return <TextField {...rest} inputRef={ref} label="Directory" />;
            }}
          />
          <Controller
            name="kind"
            control={form.control}
            render={({ field }) => {
              const { ref, ...rest } = field;
              return (
                <KindSelect
                  {...rest}
                  inputRef={ref}
                  label="Kind"
                  kinds={kinds}
                  emptyOption={{ label: "Automatic detection" }}
                  showKind
                  isKindDisabled={(kind) =>
                    !kinds.find((item) => item.kind === kind)?.supportsContent
                  }
                />
              );
            }}
          />
          {pending && progress ? <UploadProgressView progress={progress} /> : null}
          <DialogActions sx={{ px: 0, pb: 0 }}>
            <Button disabled={pending} onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" variant="contained" disabled={pending || !file}>
              {pending ? <CircularProgress size={17} color="inherit" sx={{ mr: 1 }} /> : null}
              {pending && progress ? uploadButtonLabel(progress) : "Upload"}
            </Button>
          </DialogActions>
        </Box>
      </DialogContent>
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
    preparing: "Calculating local SHA-256…",
    uploading: "Uploading file…",
    finalizing: "Verifying and publishing resource…",
  }[progress.stage];
  const finalizing = progress.stage === "finalizing";

  return (
    <Box
      sx={{
        p: 2,
        borderRadius: 2,
        border: 1,
        borderColor: "primary.light",
        bgcolor: "primary.light",
      }}
      aria-live="polite"
    >
      <Stack direction="row" justifyContent="space-between" spacing={1.5}>
        <Typography variant="body2" fontWeight={600}>
          {label}
        </Typography>
        <Typography variant="body2" fontWeight={600}>
          {finalizing ? "File uploaded" : `${percentage}%`}
        </Typography>
      </Stack>
      <LinearProgress variant="determinate" value={percentage} sx={{ mt: 1.5, mb: 1 }} />
      <Typography variant="caption" color="text.secondary">
        {formatBytes(progress.bytesSent)} / {formatBytes(progress.totalBytes)}
        {progress.stage === "uploading"
          ? " · updates after each 8 MiB chunk"
          : finalizing
            ? " transferred · large files take longer to verify"
            : " hashed locally in a background worker"}
      </Typography>
    </Box>
  );
}

function uploadButtonLabel(progress: UploadProgress): string {
  if (progress.stage === "preparing") return "Hashing…";
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
    <Dialog open={open} fullWidth maxWidth="xs" onClose={() => onOpenChange(false)}>
      <DialogTitle>New folder</DialogTitle>
      <DialogContent>
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 2 }}>
          Inside /{parent}
        </Typography>
        <Box
          component="form"
          sx={{ display: "grid", gap: 2, pt: 1 }}
          onSubmit={form.handleSubmit(async ({ name, kind }) => {
            await onCreate(name, kind || undefined);
            onOpenChange(false);
          })}
        >
          <Controller
            name="name"
            control={form.control}
            render={({ field, fieldState }) => {
              const { ref, ...rest } = field;
              return (
                <TextField
                  {...rest}
                  inputRef={ref}
                  label="Folder name"
                  autoFocus
                  error={Boolean(fieldState.error)}
                  helperText={fieldState.error?.message}
                />
              );
            }}
          />
          <Controller
            name="kind"
            control={form.control}
            render={({ field }) => {
              const { ref, ...rest } = field;
              return <KindSelect {...rest} inputRef={ref} label="Folder kind" kinds={kinds} />;
            }}
          />
          <DialogActions sx={{ px: 0, pb: 0 }}>
            <Button onClick={() => onOpenChange(false)}>Cancel</Button>
            <Button
              type="submit"
              variant="contained"
              startIcon={<CreateNewFolderIcon />}
              disabled={pending}
            >
              {pending ? "Creating…" : "Create"}
            </Button>
          </DialogActions>
        </Box>
      </DialogContent>
    </Dialog>
  );
}
