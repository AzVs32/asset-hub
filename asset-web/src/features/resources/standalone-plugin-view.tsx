import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useParams } from "react-router";
import { useGateway } from "@/application/ports/gateway-context";
import { PluginOutput } from "@/plugins/plugin-output";
import { ErrorState, LoadingState } from "@/shared/ui/state";

export function StandalonePluginView() {
  const gateway = useGateway();
  const queryClient = useQueryClient();
  const { resourceId = "", actionId = "" } = useParams();
  const result = useQuery({
    queryKey: ["standalone-plugin-view", resourceId, actionId],
    queryFn: async () => {
      const resource = await gateway.findResource(resourceId);
      const action = resource.actions.find((candidate) => candidate.id === actionId);
      if (!action) throw new Error(`Action ${actionId} is not available.`);
      if (
        action.ui.confirmation &&
        !window.confirm(action.ui.confirmation.replaceAll("{name}", resource.name))
      ) {
        throw new Error(`Action ${actionId} was not confirmed.`);
      }
      return { resource, output: await gateway.executeAction(resource, action.id) };
    },
    retry: false,
  });
  if (result.isPending) return <LoadingState label="Opening plugin view" />;
  if (result.isError) return <ErrorState error={result.error} />;
  return (
    <main className="min-h-screen bg-slate-100 p-4">
      <section className="mx-auto max-w-7xl overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-sm">
        <header className="border-b border-slate-200 px-5 py-4">
          <h1 className="font-semibold text-slate-900">{result.data.resource.name}</h1>
          <p className="text-xs text-slate-500">{actionId}</p>
        </header>
        <PluginOutput
          output={result.data.output}
          resource={result.data.resource}
          onResourceChanged={() =>
            queryClient.invalidateQueries({
              queryKey: ["standalone-plugin-view", resourceId, actionId],
            })
          }
        />
      </section>
    </main>
  );
}
