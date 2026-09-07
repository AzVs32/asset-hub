import { useMutation, useQueryClient } from "@tanstack/react-query";
import React from "react";
import { toast } from "sonner";
import type { Directory } from "@/domain/directory";
import type { Resource, ResourceDraft } from "@/domain/resource";
import { ConcurrentModificationError } from "@/shared/api/errors";
import { useAssetWorkspaceGateway } from "@/shared/api/gateway-context";
import { queryKeys } from "@/shared/api/query-keys";
import type { UploadDraft, UploadProgress } from "@/shared/api/upload";
import { refreshDirectories, refreshResource } from "../workspace-cache";

export function useAssetWorkspaceCommands() {
  const assetGateway = useAssetWorkspaceGateway();
  const queryClient = useQueryClient();
  const [uploadProgress, setUploadProgress] = React.useState<UploadProgress | null>(null);

  async function handleResourceError(error: unknown, resource: Resource) {
    if (error instanceof ConcurrentModificationError) {
      try {
        const latest = await refreshResource(queryClient, assetGateway, resource.id);
        await refreshDirectories(queryClient, [resource.directory, latest.directory]);
      } catch {
        await refreshDirectories(queryClient, [resource.directory]);
        toast.error("Unable to reload the resource. Retry loading its details before editing.");
        return;
      }
    }
    notifyError(error);
  }

  async function handleDirectoryError(error: unknown, paths: string[]) {
    if (error instanceof ConcurrentModificationError) await refreshDirectories(queryClient, paths);
    notifyError(error);
  }

  const update = useMutation({
    mutationFn: ({ resource, draft }: { resource: Resource; draft: ResourceDraft }) =>
      assetGateway.updateResource(resource, draft),
    onSuccess: async (resource, { resource: previous }) => {
      toast.success("Resource saved");
      await queryClient.cancelQueries({ queryKey: queryKeys.resource(resource.id) });
      queryClient.setQueryData(queryKeys.resource(resource.id), resource);
      await refreshDirectories(queryClient, [previous.directory, resource.directory]);
    },
    onError: (error, { resource }) => handleResourceError(error, resource),
  });
  const upload = useMutation({
    mutationFn: (draft: UploadDraft) => assetGateway.uploadResource(draft, setUploadProgress),
    onMutate: (draft) => {
      setUploadProgress({ stage: "preparing", bytesSent: 0, totalBytes: draft.file.size });
    },
    onSettled: () => setUploadProgress(null),
  });

  const createFolder = useMutation({
    mutationFn: ({ parent, name }: { parent: Directory; name: string }) =>
      assetGateway.createDirectory(parent, name),
    onSuccess: async (_directory, { parent }) => {
      toast.success("Folder created");
      await refreshDirectories(queryClient, [parent.path]);
    },
    onError: (error, { parent }) => handleDirectoryError(error, [parent.path]),
  });
  const deleteResource = useMutation({
    mutationFn: ({ resource }: { resource: Resource }) => assetGateway.deleteResource(resource),
    onSuccess: async (_data, { resource }) => {
      toast.success(`${resource.name} deleted`);
      queryClient.removeQueries({ queryKey: queryKeys.resource(resource.id) });
      await refreshDirectories(queryClient, [resource.directory]);
    },
    onError: (error, { resource }) => handleResourceError(error, resource),
  });
  const deleteDirectory = useMutation({
    mutationFn: ({ directory }: { directory: Directory }) =>
      assetGateway.deleteDirectory(directory),
    onSuccess: async (_data, { directory }) => {
      toast.success(`${directory.name} deleted`);
      await refreshDirectories(queryClient, [directory.path, directory.parentPath]);
    },
    onError: (error, { directory }) =>
      handleDirectoryError(error, [directory.path, directory.parentPath]),
  });

  return {
    update,
    upload,
    uploadProgress,
    createFolder,
    deleteResource,
    deleteDirectory,
  };
}

function notifyError(error: unknown) {
  toast.error(error instanceof Error ? error.message : "Request failed");
}
