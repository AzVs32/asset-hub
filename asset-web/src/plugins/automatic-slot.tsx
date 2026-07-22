import { useQuery } from "@tanstack/react-query";
import { useGateway } from "@/application/ports/gateway-context";
import type { WorkspaceResource } from "@/application/workspace/workspace-scope";
import { useWorkspaceScope } from "@/application/workspace/workspace-scope-context";
import type { ResourceAction } from "@/domain/resource";
import { usePluginKernel } from "@/kernel/plugin-kernel";
import type { HostSlot } from "@/kernel/slots";
import { ErrorState, LoadingState } from "@/shared/ui/state";
import { PluginOutput } from "./plugin-output";

export function AutomaticSlot({
  slot,
  resource,
  onResourceChanged,
}: {
  slot: HostSlot;
  resource: WorkspaceResource;
  onResourceChanged?: (() => void | Promise<void>) | undefined;
}) {
  const kernel = usePluginKernel();
  const actions = kernel
    .actionsAt(resource, slot)
    .filter((action) => action.access === "read_only");
  if (!actions.length) return null;
  return (
    <div className="grid gap-4" data-plugin-slot={slot}>
      {actions.map((action) => (
        <AutomaticAction
          key={action.id}
          action={action}
          resource={resource}
          onResourceChanged={onResourceChanged}
        />
      ))}
    </div>
  );
}

function AutomaticAction({
  action,
  resource,
  onResourceChanged,
}: {
  action: ResourceAction;
  resource: WorkspaceResource;
  onResourceChanged?: (() => void | Promise<void>) | undefined;
}) {
  const gateway = useGateway();
  const scope = useWorkspaceScope();
  const result = useQuery({
    queryKey: ["plugin-slot", resource.id, resource.updatedAt, action.id],
    queryFn: () => gateway.executeAction(scope.toStorageResource(resource), action.id),
    retry: false,
  });
  return (
    <section className="overflow-hidden rounded-2xl border border-slate-200 bg-white">
      <header className="border-b border-slate-100 px-4 py-3">
        <h3 className="text-sm font-semibold text-slate-900">{action.label}</h3>
      </header>
      {result.isPending ? <LoadingState label={`Loading ${action.label}`} compact /> : null}
      {result.isError ? <ErrorState error={result.error} compact /> : null}
      {result.data ? (
        <PluginOutput
          output={result.data}
          resource={resource}
          onResourceChanged={onResourceChanged}
        />
      ) : null}
    </section>
  );
}
