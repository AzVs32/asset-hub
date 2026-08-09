import { useMutation, useQueryClient } from "@tanstack/react-query";
import React from "react";
import { toast } from "sonner";
import { ConcurrentModificationError } from "@/application/errors";
import { useGateway } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
import type {
  Directory,
  DirectoryAction,
  Resource,
  ResourceAction,
  ResourceDraft,
  UploadDraft,
  UploadProgress,
} from "@/domain/resource";
import type { DirectoryActionResult } from "@/plugins/directory-action-dialog";
import type { ActionResult } from "@/plugins/plugin-action-dialog";

export function useResourceCommands() {
  const gateway = useGateway();
  const queryClient = useQueryClient();
  const [actionResult, setActionResult] = React.useState<ActionResult | null>(null);
  const [directoryActionResult, setDirectoryActionResult] =
    React.useState<DirectoryActionResult | null>(null);
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
      gateway.updateResource(resource, draft),
    onSuccess: async (resource) => {
      toast.success("Resource saved");
      queryClient.setQueryData(queryKeys.resource(resource.id), resource);
      await refresh(resource.id);
    },
    onError: handleMutationError,
  });
  const upload = useMutation({
    mutationFn: (draft: UploadDraft) => gateway.uploadResource(draft, setUploadProgress),
    onMutate: (draft) => {
      setUploadProgress({ stage: "preparing", bytesSent: 0, totalBytes: draft.file.size });
    },
    onSuccess: (receipt) => {
      setUploadProgress(null);
      const notification = toast.loading(
        `${receipt.name} uploaded; verifying and publishing in the background`,
      );
      void gateway
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
  const remove = useMutation({
    mutationFn: (resource: Resource) => gateway.deleteResource(resource),
    onSuccess: async (resource) => {
      toast.success(`${resource.name} moved to deleted resources`);
      await refresh(resource.id);
    },
    onError: handleMutationError,
  });
  const restore = useMutation({
    mutationFn: (resource: Resource) => gateway.restoreResource(resource),
    onSuccess: async (resource) => {
      toast.success(`${resource.name} restored`);
      await refresh(resource.id);
    },
    onError: handleMutationError,
  });
  const createFolder = useMutation({
    mutationFn: ({ parent, name, kind }: { parent: Directory; name: string; kind?: string }) =>
      gateway.createDirectory(parent, name, kind),
    onSuccess: async () => {
      toast.success("Folder created");
      await refresh();
    },
    onError: handleMutationError,
  });
  const execute = useMutation({
    mutationFn: async ({ resource, action }: { resource: Resource; action: ResourceAction }) => ({
      resource,
      action,
      output: await gateway.executeAction(resource, action.id),
    }),
    onSuccess: async (result) => {
      setActionResult(result);
      if (result.action.access === "write") await refresh(result.resource.id);
    },
    onError: handleMutationError,
  });
  const executeDirectory = useMutation({
    mutationFn: async ({
      directory,
      action,
    }: {
      directory: Directory;
      action: DirectoryAction;
    }) => ({
      directory,
      action,
      output: await gateway.executeDirectoryAction(directory, action),
    }),
    onSuccess: async (result) => {
      setDirectoryActionResult(result);
      if (result.action.access === "write") await refresh();
    },
    onError: handleMutationError,
  });

  return {
    update,
    upload,
    uploadProgress,
    remove,
    restore,
    createFolder,
    execute,
    executeDirectory,
    actionResult,
    setActionResult,
    directoryActionResult,
    setDirectoryActionResult,
    refresh,
  };
}

function notifyError(error: unknown) {
  toast.error(error instanceof Error ? error.message : "Request failed");
}
