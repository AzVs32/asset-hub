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
    <main className="min-h-screen bg-[#eef2f7] p-4 xl:p-6">
      <section className="mx-auto max-w-7xl overflow-hidden rounded-3xl border border-slate-200/80 bg-white shadow-[0_24px_70px_-36px_rgba(15,23,42,0.5)]">
        <header className="border-b border-slate-100 bg-slate-950 px-6 py-5 text-white">
          <p className="text-[10px] font-bold uppercase tracking-[0.15em] text-indigo-300">
            Plugin view
          </p>
          <h1 className="mt-1 font-bold tracking-[-0.02em]">{result.data.resource.name}</h1>
          <p className="mt-1 text-xs text-slate-400">{actionId}</p>
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
