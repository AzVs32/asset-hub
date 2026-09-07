import { Alert, Button, Stack } from "@mui/material";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import React from "react";
import { toast } from "sonner";
import { useAssetWorkspaceGateway } from "@/shared/api/gateway-context";
import { queryKeys } from "@/shared/api/query-keys";
import type { UploadReceipt } from "@/shared/api/upload";
import { refreshDirectories } from "../workspace-cache";

export function UploadStatusList({
  uploads,
  onComplete,
}: {
  uploads: UploadReceipt[];
  onComplete: (id: string) => void;
}) {
  if (!uploads.length) return null;
  return (
    <Stack
      spacing={1}
      sx={{ px: 2, pt: 2, maxHeight: "25vh", overflow: "auto" }}
      aria-live="polite"
    >
      {uploads.map((upload) => (
        <UploadStatus key={upload.id} upload={upload} onComplete={onComplete} />
      ))}
    </Stack>
  );
}

function UploadStatus({
  upload,
  onComplete,
}: {
  upload: UploadReceipt;
  onComplete: (id: string) => void;
}) {
  const gateway = useAssetWorkspaceGateway();
  const client = useQueryClient();
  const acknowledged = React.useRef(false);
  const query = useQuery({
    queryKey: queryKeys.upload(upload.id),
    queryFn: ({ signal }) => gateway.uploadStatus(upload.id, signal),
    retry: 3,
    retryDelay: (attempt) => Math.min(1_000 * 2 ** attempt, 30_000),
    refetchOnWindowFocus: true,
    refetchInterval: (state) => {
      if (state.state.error) return 30_000;
      return state.state.data?.status === "finalizing" ? 1_000 : false;
    },
  });

  React.useEffect(() => {
    if (query.data?.status !== "completed" || acknowledged.current) return;
    acknowledged.current = true;
    const resource = query.data.resource;
    // Complete this sequence even if the panel unmounts after publication.
    void (async () => {
      await client.cancelQueries({ queryKey: queryKeys.resource(resource.id) });
      client.setQueryData(queryKeys.resource(resource.id), resource);
      await refreshDirectories(client, [resource.directory]);
      toast.success(`${resource.name} is ready`);
      onComplete(upload.id);
      await gateway.acknowledgeUpload(upload.id);
    })();
  }, [query.data, client, gateway, onComplete, upload.id]);

  const retrying = query.isError || query.failureCount > 0;
  const status = query.data;
  const message = retrying
    ? "Unable to check publishing status. Retrying automatically; your upload may still be processing."
    : status?.status === "failed"
      ? status.message
      : status?.status === "uploading"
        ? "Upload is incomplete. Choose the same file and destination to resume."
        : "Verifying and publishing…";
  return (
    <Alert
      severity={retrying ? "warning" : status?.status === "failed" ? "error" : "info"}
      action={
        retrying || status?.status === "uploading" || status?.status === "failed" ? (
          <Button
            color="inherit"
            size="small"
            disabled={query.isFetching}
            onClick={() => void query.refetch()}
          >
            Check again
          </Button>
        ) : undefined
      }
    >
      {upload.name}: {message}
    </Alert>
  );
}
