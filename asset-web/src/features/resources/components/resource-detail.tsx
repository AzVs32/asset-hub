import { zodResolver } from "@hookform/resolvers/zod";
import SaveIcon from "@mui/icons-material/Save";
import StorageRoundedIcon from "@mui/icons-material/StorageRounded";
import {
  Avatar,
  Box,
  Button,
  Card,
  CardContent,
  CardHeader,
  Chip,
  Divider,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import React from "react";
import { Controller, useForm } from "react-hook-form";
import { z } from "zod";
import type { Resource, ResourceDraft, ResourceKind } from "@/domain/resource";
import { draftFromResource, formatBytes, formatDate } from "@/domain/resource-draft";
import { ResourceThumbnail } from "./asset-thumbnail";
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
      <Card sx={{ minHeight: 288, display: "grid", placeItems: "center" }}>
        <Stack alignItems="center" spacing={1.5}>
          <Avatar sx={{ width: 56, height: 56 }}>
            <StorageRoundedIcon />
          </Avatar>
          <Typography variant="body2" color="text.secondary">
            Select an asset or folder to see its details
          </Typography>
        </Stack>
      </Card>
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
    <Card sx={{ minHeight: 0, overflow: "auto" }}>
      <CardHeader
        avatar={<ResourceThumbnail resource={resource} size={64} />}
        title={resource.name}
        subheader={resource.id}
        action={
          resource.deletedAt ? <Chip label="deleted" color="error" size="small" /> : undefined
        }
      />
      <Divider />
      <CardContent>
        <Stack spacing={2}>
          <Stack component="form" spacing={2} onSubmit={form.handleSubmit(onSave)}>
            <Controller
              name="name"
              control={form.control}
              render={({ field, fieldState }) => {
                const { ref, ...rest } = field;
                return (
                  <TextField
                    {...rest}
                    inputRef={ref}
                    label="Name"
                    disabled={Boolean(resource.deletedAt)}
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
                const { ref, ...rest } = field;
                return (
                  <TextField
                    {...rest}
                    inputRef={ref}
                    label="Directory"
                    disabled={Boolean(resource.deletedAt)}
                  />
                );
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
                    disabled={Boolean(resource.deletedAt)}
                    error={Boolean(fieldState.error)}
                    helperText={fieldState.error?.message}
                  />
                );
              }}
            />
            <Box sx={{ display: "flex", justifyContent: "flex-end" }}>
              <Button
                type="submit"
                variant="contained"
                size="small"
                startIcon={<SaveIcon />}
                disabled={pending || Boolean(resource.deletedAt) || !form.formState.isDirty}
              >
                Save
              </Button>
            </Box>
          </Stack>
          <Divider />
          <Box sx={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 2 }}>
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
            <Fact
              label="Kind origin"
              value={kind ? `${kind.origin.kind}:${kind.origin.id}` : "—"}
            />
            <Fact
              label="Content"
              value={
                resource.content
                  ? resource.directory
                    ? `${resource.directory}/${resource.name}`
                    : `/${resource.name}`
                  : "—"
              }
            />
            <Fact
              label="Actions"
              value={resource.actions.map((action) => action.id).join(", ") || "—"}
            />
          </Box>
        </Stack>
      </CardContent>
    </Card>
  );
}

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <Box>
      <Typography variant="overline" color="text.secondary">
        {label}
      </Typography>
      <Typography variant="body2" sx={{ wordBreak: "break-word" }}>
        {value}
      </Typography>
    </Box>
  );
}
