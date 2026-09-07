import { zodResolver } from "@hookform/resolvers/zod";
import CloseIcon from "@mui/icons-material/Close";
import EditIcon from "@mui/icons-material/Edit";
import SaveIcon from "@mui/icons-material/Save";
import StorageRoundedIcon from "@mui/icons-material/StorageRounded";
import { Alert, Avatar, Box, Button, CircularProgress, Stack, TextField } from "@mui/material";
import React from "react";
import { Controller, useForm } from "react-hook-form";
import { z } from "zod";
import type { Resource, ResourceDraft } from "@/domain/resource";
import { draftFromResource } from "@/domain/resource-draft";
import { ConcurrentModificationError } from "@/shared/api/errors";
import { formatBytes, formatDate } from "@/shared/format";
import { ResourceThumbnail } from "./asset-thumbnail";
import {
  CopyableValue,
  DetailAdvanced,
  DetailEmptyState,
  DetailPanel,
  DetailRow,
  DetailSection,
} from "./detail-panel";
import { DirectorySelect } from "./directory-select";

const draftSchema = z.object({
  name: z.string().refine((value) => value.trim().length > 0, "Name is required"),
  directory: z.string(),
});

interface ResourceDetailProps {
  resource: Resource | null;
  pending: boolean;
  selected: boolean;
  loading: boolean;
  error: unknown;
  onRetry: () => void;
  onSave: (resource: Resource, draft: ResourceDraft) => Promise<unknown>;
}

export function ResourceDetail({ resource, selected, loading, ...props }: ResourceDetailProps) {
  if (!resource && selected) {
    return (
      <Stack sx={{ p: 2 }} spacing={2}>
        {props.error ? (
          <Alert
            severity="error"
            action={
              <Button color="inherit" onClick={props.onRetry}>
                Retry
              </Button>
            }
          >
            {props.error instanceof Error ? props.error.message : "Resource is unavailable"}
          </Alert>
        ) : loading ? (
          <CircularProgress aria-label="Loading resource" />
        ) : (
          <Alert severity="info">Resource is unavailable.</Alert>
        )}
      </Stack>
    );
  }
  if (!resource) {
    return (
      <DetailEmptyState
        icon={
          <Avatar sx={{ width: 52, height: 52 }}>
            <StorageRoundedIcon />
          </Avatar>
        }
        message="Select an asset or folder to inspect its details"
      />
    );
  }
  return <Detail key={resource.id} resource={resource} {...props} />;
}

function Detail({
  resource,
  pending,
  error,
  onRetry,
  onSave,
}: Omit<ResourceDetailProps, "selected" | "loading" | "resource"> & { resource: Resource }) {
  const [snapshot, setSnapshot] = React.useState<Resource | null>(null);
  const [requiresReload, setRequiresReload] = React.useState(false);
  const editing = snapshot !== null;
  const stale = requiresReload || (snapshot !== null && snapshot.revision !== resource.revision);
  const blocked =
    pending || Boolean(error) || stale || resource.state.lifecycle.status !== "active";
  const form = useForm<ResourceDraft>({
    resolver: zodResolver(draftSchema),
    defaultValues: draftFromResource(resource),
  });
  function loadDraft() {
    form.reset(draftFromResource(resource));
    setSnapshot(resource);
    setRequiresReload(false);
  }

  const resourcePath = resource.directory
    ? `/${resource.directory}/${resource.name}`
    : `/${resource.name}`;

  return (
    <DetailPanel
      thumbnail={<ResourceThumbnail size={48} />}
      title={resource.name}
      subtitle={resourcePath}
      action={
        !editing && resource.state.lifecycle.status === "active" ? (
          <Button
            size="small"
            disabled={Boolean(error)}
            startIcon={<EditIcon />}
            onClick={loadDraft}
          >
            Edit
          </Button>
        ) : undefined
      }
    >
      {error ? (
        <Alert
          severity="error"
          action={
            <Button color="inherit" onClick={onRetry}>
              Retry
            </Button>
          }
        >
          Unable to load the latest resource. Reload before editing.
        </Alert>
      ) : null}
      {editing ? (
        <DetailSection title="Edit resource" divider={false}>
          <Box
            component="form"
            onSubmit={form.handleSubmit(async (draft) => {
              if (!snapshot || blocked) return;
              try {
                await onSave(snapshot, draft);
                setSnapshot(null);
              } catch (failure) {
                if (failure instanceof ConcurrentModificationError) setRequiresReload(true);
                form.setError("root", {
                  message: failure instanceof Error ? failure.message : "Save failed",
                });
              }
            })}
          >
            <Stack spacing={2}>
              {stale && !pending ? (
                <Alert
                  severity="warning"
                  action={
                    <Button color="inherit" disabled={Boolean(error)} onClick={loadDraft}>
                      Reload latest
                    </Button>
                  }
                >
                  This resource changed. Your draft is preserved below; reload the latest version
                  before editing again.
                </Alert>
              ) : null}
              {form.formState.errors.root ? (
                <Alert severity="error">{form.formState.errors.root.message}</Alert>
              ) : null}
              <Controller
                name="name"
                control={form.control}
                render={({ field, fieldState }) => {
                  const { ref, ...rest } = field;
                  return (
                    <TextField
                      {...rest}
                      inputRef={ref}
                      disabled={blocked}
                      label="Resource name"
                      autoFocus
                      error={Boolean(fieldState.error)}
                      helperText={fieldState.error?.message}
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
                    <DirectorySelect
                      {...rest}
                      disabled={blocked}
                      inputRef={ref}
                      onChange={onChange}
                    />
                  );
                }}
              />
              <Stack direction="row" justifyContent="flex-end" spacing={1}>
                <Button
                  startIcon={<CloseIcon />}
                  disabled={pending}
                  onClick={() => {
                    form.reset(draftFromResource(resource));
                    setSnapshot(null);
                  }}
                >
                  Cancel
                </Button>
                <Button
                  type="submit"
                  variant="contained"
                  startIcon={<SaveIcon />}
                  disabled={blocked || !form.formState.isDirty}
                >
                  {pending ? "Saving…" : "Save"}
                </Button>
              </Stack>
            </Stack>
          </Box>
        </DetailSection>
      ) : (
        <>
          <DetailSection title="General">
            <DetailRow label="Size">{formatBytes(resource.content?.size)}</DetailRow>
            <DetailRow label="Status">{resource.state.effective}</DetailRow>
            {resource.content?.verificationError ? (
              <Alert severity="error">{resource.content.verificationError}</Alert>
            ) : null}
            <DetailRow label="MIME">{resource.content?.mimeType ?? "—"}</DetailRow>
            <DetailRow label="Created">{formatDate(resource.createdAt)}</DetailRow>
            <DetailRow label="Updated">{formatDate(resource.updatedAt)}</DetailRow>
          </DetailSection>
          <DetailAdvanced>
            <DetailRow label="Resource ID">
              <CopyableValue value={resource.id} />
            </DetailRow>
            <DetailRow label="Revision">{resource.revision}</DetailRow>
            <DetailRow label="Checksum">
              {resource.content?.checksum ? (
                <CopyableValue
                  value={`${resource.content.checksum.kind}:${resource.content.checksum.value}`}
                />
              ) : (
                "—"
              )}
            </DetailRow>
          </DetailAdvanced>
        </>
      )}
    </DetailPanel>
  );
}
