import { useMutation, useQueryClient } from "@tanstack/react-query";
import React from "react";
import { toast } from "sonner";
import type { Directory } from "@/domain/directory";
import type { Resource, ResourceDraft, UploadDraft, UploadProgress } from "@/domain/resource";
import { ConcurrentModificationError } from "@/shared/api/errors";
import { useAssetWorkspaceGateway } from "@/shared/api/gateway-context";
import { queryKeys } from "@/shared/api/query-keys";

export function useAssetWorkspaceCommands() {
  const assetGateway = useAssetWorkspaceGateway();
  const queryClient = useQueryClient();
  const [uploadProgress, setUploadProgress] = React.useState<UploadProgress | null>(null);

  const refresh = React.useCallback(
    async (resourceId?: string) => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["directory"] }),
        ...(resourceId
          ? [queryClient.invalidateQueries({ queryKey: queryKeys.resource(resourceId) })]
          : []),
      ]);
    },
    [queryClient],
  );
  const handleMutationError = React.useCallback(
    async (error: unknown) => {
      if (error instanceof ConcurrentModificationError) await refresh();
      notifyError(error);
    },
    [refresh],
  );

  const update = useMutation({
    mutationFn: ({ resource, draft }: { resource: Resource; draft: ResourceDraft }) =>
      assetGateway.updateResource(resource, draft),
    onSuccess: async (resource) => {
      toast.success("Resource saved");
      queryClient.setQueryData(queryKeys.resource(resource.id), resource);
      await refresh(resource.id);
    },
    onError: handleMutationError,
  });
  const upload = useMutation({
    mutationFn: (draft: UploadDraft) => assetGateway.uploadResource(draft, setUploadProgress),
    onMutate: (draft) => {
      setUploadProgress({ stage: "preparing", bytesSent: 0, totalBytes: draft.file.size });
    },
    onSuccess: (receipt) => {
      setUploadProgress(null);
      const notification = toast.loading(
        `${receipt.name} uploaded; verifying and publishing in the background`,
      );
      void assetGateway
        .waitForUpload(receipt.id)
        .then(async (resource) => {
          toast.success(`${resource.name} is ready`, { id: notification });
          await refresh(resource.id);
        })
        .catch((error) => {
          toast.error(error instanceof Error ? error.message : "Resource publishing failed", {
            id: notification,
          });
        });
    },
    onError: (error) => {
      setUploadProgress(null);
      void handleMutationError(error);
    },
  });
  const createFolder = useMutation({
    mutationFn: ({ parent, name, kind }: { parent: Directory; name: string; kind?: string }) =>
      assetGateway.createDirectory(parent, name, kind),
    onSuccess: async () => {
      toast.success("Folder created");
      await refresh();
    },
    onError: handleMutationError,
  });
  const updateDirectoryKind = useMutation({
    mutationFn: ({ directory, kind }: { directory: Directory; kind: string }) => {
      if (!directory.parentId) throw new Error("The root directory kind cannot be changed");
      return assetGateway.updateDirectory(directory, { kind });
    },
    onSuccess: async (directory) => {
      toast.success(`${directory.name} kind changed`);
      await refresh();
    },
    onError: handleMutationError,
  });
  const deleteResource = useMutation({
    mutationFn: ({ resource }: { resource: Resource }) => assetGateway.deleteResource(resource),
    onSuccess: async (_data, { resource }) => {
      toast.success(`${resource.name} deleted`);
      queryClient.removeQueries({ queryKey: queryKeys.resource(resource.id) });
      await refresh();
    },
    onError: handleMutationError,
  });
  const deleteDirectory = useMutation({
    mutationFn: ({ directory }: { directory: Directory }) =>
      assetGateway.deleteDirectory(directory),
    onSuccess: async (_data, { directory }) => {
      toast.success(`${directory.name} deleted`);
      await refresh();
    },
    onError: handleMutationError,
  });

  return {
    update,
    upload,
    uploadProgress,
    createFolder,
    updateDirectoryKind,
    deleteResource,
    deleteDirectory,
    refresh,
  };
}

function notifyError(error: unknown) {
  toast.error(error instanceof Error ? error.message : "Request failed");
}
