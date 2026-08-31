import { zodResolver } from "@hookform/resolvers/zod";
import CloseIcon from "@mui/icons-material/Close";
import EditIcon from "@mui/icons-material/Edit";
import SaveIcon from "@mui/icons-material/Save";
import StorageRoundedIcon from "@mui/icons-material/StorageRounded";
import { Avatar, Box, Button, Chip, Stack, TextField } from "@mui/material";
import React from "react";
import { Controller, useForm } from "react-hook-form";
import { z } from "zod";
import type { Resource, ResourceDraft, ResourceKind } from "@/domain/resource";
import { draftFromResource } from "@/domain/resource-draft";
import { formatBytes, formatDate } from "@/shared/format";
import { ResourceThumbnail } from "./asset-thumbnail";
import {
  CopyableValue,
  DetailAdvanced,
  DetailEmptyState,
  DetailPanel,
  DetailRow,
  DetailSection,
  DetailValueList,
} from "./detail-panel";
import { DirectorySelect } from "./directory-select";
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
  return (
    <Detail key={resource.id} resource={resource} kinds={kinds} pending={pending} onSave={onSave} />
  );
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
  const [editing, setEditing] = React.useState(false);
  const form = useForm<ResourceDraft>({
    resolver: zodResolver(draftSchema),
    defaultValues: draftFromResource(resource),
  });
  React.useEffect(() => {
    form.reset(draftFromResource(resource));
  }, [form, resource]);

  const kind = kinds.find((candidate) => candidate.kind === resource.kind);
  const resourcePath = resource.directory
    ? `/${resource.directory}/${resource.name}`
    : `/${resource.name}`;

  return (
    <DetailPanel
      thumbnail={<ResourceThumbnail resource={resource} size={48} />}
      title={resource.name}
      subtitle={resourcePath}
      badges={<Chip label={kind?.label ?? resource.kind} size="small" variant="outlined" />}
      action={
        !editing && resource.state.lifecycle.status === "active" ? (
          <Button size="small" startIcon={<EditIcon />} onClick={() => setEditing(true)}>
            Edit
          </Button>
        ) : undefined
      }
    >
      {editing ? (
        <DetailSection title="Edit resource" divider={false}>
          <Box
            component="form"
            onSubmit={form.handleSubmit(async (draft) => {
              await onSave(draft);
              setEditing(false);
            })}
          >
            <Stack spacing={2}>
              <Controller
                name="name"
                control={form.control}
                render={({ field, fieldState }) => {
                  const { ref, ...rest } = field;
                  return (
                    <TextField
                      {...rest}
                      inputRef={ref}
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
                  return <DirectorySelect {...rest} inputRef={ref} onChange={onChange} />;
                }}
              />
              <Controller
                name="kind"
                control={form.control}
                render={({ field, fieldState }) => {
                  const { ref, ...rest } = field;
                  return (
                    <KindSelect
                      {...rest}
                      inputRef={ref}
                      label="Kind"
                      kinds={kinds}
                      showKind
                      error={Boolean(fieldState.error)}
                      helperText={fieldState.error?.message}
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
                    setEditing(false);
                  }}
                >
                  Cancel
                </Button>
                <Button
                  type="submit"
                  variant="contained"
                  startIcon={<SaveIcon />}
                  disabled={pending || !form.formState.isDirty}
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
            <DetailRow label="Size">{formatBytes(resource.content?.size ?? 0)}</DetailRow>
            <DetailRow label="MIME">{resource.content?.mimeType ?? "—"}</DetailRow>
            <DetailRow label="Created">{formatDate(resource.createdAt)}</DetailRow>
            <DetailRow label="Updated">{formatDate(resource.updatedAt)}</DetailRow>
            {resource.state.lifecycle.status === "deleted" ? (
              <DetailRow label="Deleted">{formatDate(resource.state.lifecycle.at)}</DetailRow>
            ) : null}
          </DetailSection>
          <DetailAdvanced>
            <DetailRow label="Resource ID">
              <CopyableValue value={resource.id} />
            </DetailRow>
            <DetailRow label="Kind ID">
              <CopyableValue value={resource.kind} />
            </DetailRow>
            <DetailRow label="Kind origin">
              {kind ? `${kind.origin.kind}:${kind.origin.id}` : "—"}
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
            <DetailRow label="Actions">
              <DetailValueList values={resource.actions.map((action) => action.id)} />
            </DetailRow>
          </DetailAdvanced>
        </>
      )}
    </DetailPanel>
  );
}
