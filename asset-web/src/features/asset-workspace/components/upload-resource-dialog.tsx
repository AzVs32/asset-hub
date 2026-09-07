import UploadFileIcon from "@mui/icons-material/UploadFile";
import {
  Alert,
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
import type { UploadDraft, UploadProgress } from "@/shared/api/upload";
import { formatBytes } from "@/shared/format";

import { DirectorySelect } from "./directory-select";

interface UploadForm {
  file: FileList;
  name: string;
  directory: string;
}

export function UploadResourceDialog({
  open,
  onOpenChange,
  directory,
  pending,
  progress,
  onUpload,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  directory: string;
  pending: boolean;
  progress: UploadProgress | null;
  onUpload: (draft: UploadDraft) => Promise<unknown>;
}) {
  const form = useForm<UploadForm>({
    defaultValues: { name: "", directory },
  });
  React.useEffect(() => {
    if (open) form.reset({ name: "", directory });
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
            try {
              await onUpload({ file: selected, name: input.name, directory: input.directory });
              onOpenChange(false);
            } catch (error) {
              form.setError("root", {
                message: error instanceof Error ? error.message : "Upload failed",
              });
            }
          })}
        >
          <Button
            disabled={pending}
            component="label"
            variant="outlined"
            startIcon={<UploadFileIcon />}
            sx={{ minHeight: 128 }}
          >
            {file?.name ?? "Choose a file"}
            <input
              disabled={pending}
              type="file"
              hidden
              {...form.register("file", {
                required: true,
                onChange: (event: React.ChangeEvent<HTMLInputElement>) => {
                  form.setValue("name", event.target.files?.item(0)?.name ?? "");
                },
              })}
            />
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
                  disabled={pending}
                  label="Resource name"
                  placeholder={file?.name ?? "Defaults to filename"}
                />
              );
            }}
          />
          <Controller
            name="directory"
            control={form.control}
            render={({ field }) => {
              const { onChange, ref, ...rest } = field;
              return (
                <DirectorySelect {...rest} inputRef={ref} onChange={onChange} disabled={pending} />
              );
            }}
          />
          {pending && progress ? <UploadProgressView progress={progress} /> : null}
          {form.formState.errors.root ? (
            <Alert severity="error">{form.formState.errors.root.message}</Alert>
          ) : null}
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
        borderColor: "#c7d2fe",
        bgcolor: "#eef2ff",
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
