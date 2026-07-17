import { useMutation, useQueryClient } from "@tanstack/react-query";
import React from "react";
import { toast } from "sonner";
import { useGateway } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
import type {
  Resource,
  ResourceAction,
  ResourceDraft,
  ResourceKindMetadataPatch,
  UploadDraft,
} from "@/domain/resource";
import type { ActionResult } from "@/plugins/plugin-action-dialog";

export function useResourceCommands() {
  const gateway = useGateway();
  const queryClient = useQueryClient();
  const [actionResult, setActionResult] = React.useState<ActionResult | null>(null);

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

  const create = useMutation({
    mutationFn: (draft: ResourceDraft) => gateway.createResource(draft),
    onSuccess: async (resource) => {
      toast.success(`Created ${resource.name}`);
      await refresh(resource.id);
    },
    onError: notifyError,
  });
  const update = useMutation({
    mutationFn: ({ id, draft }: { id: string; draft: ResourceDraft }) =>
      gateway.updateResource(id, draft),
    onSuccess: async (resource) => {
      toast.success("Resource saved");
      queryClient.setQueryData(queryKeys.resource(resource.id), resource);
      await refresh(resource.id);
    },
    onError: notifyError,
  });
  const patchKindMetadata = useMutation({
    mutationFn: ({ id, patch }: { id: string; patch: ResourceKindMetadataPatch }) =>
      gateway.patchKindMetadata(id, patch),
    onSuccess: async (resource) => {
      toast.success("Kind metadata saved");
      queryClient.setQueryData(queryKeys.resource(resource.id), resource);
      await refresh(resource.id);
    },
    onError: notifyError,
  });
  const upload = useMutation({
    mutationFn: (draft: UploadDraft) => gateway.uploadResource(draft),
    onSuccess: async (resource) => {
      toast.success(`Uploaded ${resource.name}`);
      await refresh(resource.id);
    },
    onError: notifyError,
  });
  const remove = useMutation({
    mutationFn: (resource: Resource) => gateway.deleteResource(resource.id),
    onSuccess: async (resource) => {
      toast.success(`${resource.name} moved to deleted resources`);
      await refresh(resource.id);
    },
    onError: notifyError,
  });
  const restore = useMutation({
    mutationFn: (resource: Resource) => gateway.restoreResource(resource.id),
    onSuccess: async (resource) => {
      toast.success(`${resource.name} restored`);
      await refresh(resource.id);
    },
    onError: notifyError,
  });
  const createFolder = useMutation({
    mutationFn: ({ parent, name }: { parent: string; name: string }) =>
      gateway.createDirectory(parent, name),
    onSuccess: async () => {
      toast.success("Folder created");
      await refresh();
    },
    onError: notifyError,
  });
  const scan = useMutation({
    mutationFn: (directory: string) => gateway.scan(directory),
    onSuccess: async (result) => {
      toast.success(`Scan complete: ${result.imported} imported, ${result.skipped} skipped`);
      await refresh();
    },
    onError: notifyError,
  });
  const execute = useMutation({
    mutationFn: async ({ resource, action }: { resource: Resource; action: ResourceAction }) => ({
      resource,
      action,
      output: await gateway.executeAction(resource, action.id),
    }),
    onSuccess: async (result) => {
      setActionResult(result);
      if (result.action.access === "read_write") await refresh(result.resource.id);
    },
    onError: notifyError,
  });

  return {
    create,
    update,
    patchKindMetadata,
    upload,
    remove,
    restore,
    createFolder,
    scan,
    execute,
    actionResult,
    setActionResult,
    refresh,
  };
}

function notifyError(error: unknown) {
  toast.error(error instanceof Error ? error.message : "Request failed");
}
