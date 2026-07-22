import { useMutation, useQueryClient } from "@tanstack/react-query";
import React from "react";
import { toast } from "sonner";
import { useGateway } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
import type {
  WorkspaceResource,
  WorkspaceResourceDraft,
  WorkspaceUploadDraft,
} from "@/application/workspace/workspace-scope";
import { useWorkspaceScope } from "@/application/workspace/workspace-scope-context";
import type { ResourceAction } from "@/domain/resource";
import type { ActionResult } from "@/plugins/plugin-action-dialog";

export function useResourceCommands() {
  const gateway = useGateway();
  const scope = useWorkspaceScope();
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
    mutationFn: async (draft: WorkspaceResourceDraft) =>
      scope.toVisibleResource(await gateway.createResource(scope.toStorageResourceDraft(draft))),
    onSuccess: async (resource) => {
      toast.success(`Created ${resource.name}`);
      await refresh(resource.id);
    },
    onError: notifyError,
  });
  const update = useMutation({
    mutationFn: async ({ id, draft }: { id: string; draft: WorkspaceResourceDraft }) =>
      scope.toVisibleResource(
        await gateway.updateResource(id, scope.toStorageResourceDraft(draft)),
      ),
    onSuccess: async (resource) => {
      toast.success("Resource saved");
      queryClient.setQueryData(queryKeys.resource(resource.id), resource);
      await refresh(resource.id);
    },
    onError: notifyError,
  });
  const upload = useMutation({
    mutationFn: async (draft: WorkspaceUploadDraft) =>
      scope.toVisibleResource(await gateway.uploadResource(scope.toStorageUploadDraft(draft))),
    onSuccess: async (resource) => {
      toast.success(`Uploaded ${resource.name}`);
      await refresh(resource.id);
    },
    onError: notifyError,
  });
  const remove = useMutation({
    mutationFn: async (resource: WorkspaceResource) =>
      scope.toVisibleResource(await gateway.deleteResource(resource.id)),
    onSuccess: async (resource) => {
      toast.success(`${resource.name} moved to deleted resources`);
      await refresh(resource.id);
    },
    onError: notifyError,
  });
  const restore = useMutation({
    mutationFn: async (resource: WorkspaceResource) =>
      scope.toVisibleResource(await gateway.restoreResource(resource.id)),
    onSuccess: async (resource) => {
      toast.success(`${resource.name} restored`);
      await refresh(resource.id);
    },
    onError: notifyError,
  });
  const createFolder = useMutation({
    mutationFn: ({ parent, name }: { parent: string; name: string }) =>
      gateway.createDirectory(scope.toStorageDirectory(parent), name),
    onSuccess: async () => {
      toast.success("Folder created");
      await refresh();
    },
    onError: notifyError,
  });
  const execute = useMutation({
    mutationFn: async ({
      resource,
      action,
    }: {
      resource: WorkspaceResource;
      action: ResourceAction;
    }) => ({
      resource,
      action,
      output: await gateway.executeAction(scope.toStorageResource(resource), action.id),
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
    upload,
    remove,
    restore,
    createFolder,
    execute,
    actionResult,
    setActionResult,
    refresh,
  };
}

function notifyError(error: unknown) {
  toast.error(error instanceof Error ? error.message : "Request failed");
}
