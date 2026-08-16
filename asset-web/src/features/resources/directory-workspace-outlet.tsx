import { useQuery } from "@tanstack/react-query";
import type React from "react";
import { useGateway } from "@/application/ports/gateway-context";
import type { Directory } from "@/domain/resource";
import { usePluginKernel } from "@/kernel/plugin-kernel";
import { DirectoryPluginFrame } from "@/plugins/directory-plugin-frame";
import { ErrorState, LoadingState } from "@/shared/ui/state";

export function DirectoryWorkspaceOutlet({
  directory,
  coreWorkspace,
  onDirectoryChanged,
  onNavigate,
}: {
  directory: Directory | undefined;
  coreWorkspace: React.ReactNode;
  onDirectoryChanged: () => void | Promise<void>;
  onNavigate: (path: string) => void | Promise<void>;
}) {
  const gateway = useGateway();
  const kernel = usePluginKernel();
  const action = directory ? kernel.directoryWorkspaceAction(directory) : null;
  const result = useQuery({
    queryKey: ["directory-workspace", directory?.id, directory?.revision, action?.id],
    queryFn: () => {
      if (!directory || !action) throw new Error("Directory workspace provider is unavailable.");
      return gateway.executeDirectoryAction(directory, action.id);
    },
    enabled: Boolean(directory && action),
    retry: false,
  });

  if (!directory) return <LoadingState label="Loading directory" />;
  if (!action) return coreWorkspace;
  if (result.error) return <ErrorState error={result.error} />;
  if (!result.data) return <LoadingState label="Loading directory workspace" />;
  const view = result.data.view;
  if (view?.view !== "plugin_frame") {
    return <ErrorState error={new Error(`Action ${action.id} did not return a plugin_frame.`)} />;
  }
  return (
    <section
      className="m-4 flex min-h-0 flex-1 flex-col overflow-hidden rounded-3xl border border-slate-200/80 bg-white shadow-[0_18px_50px_-30px_rgba(15,23,42,0.45)] xl:m-5"
      aria-label={action.label}
    >
      {result.data.diagnostics.length ? (
        <div className="m-4 rounded-2xl border border-amber-200 bg-amber-50 p-4 text-sm text-amber-950 shadow-sm">
          {result.data.diagnostics.map((diagnostic) => (
            <p key={`${diagnostic.code}:${diagnostic.message}`}>{diagnostic.message}</p>
          ))}
        </div>
      ) : null}
      <DirectoryPluginFrame
        directory={directory}
        output={result.data}
        view={view}
        gateway={gateway}
        onDirectoryChanged={onDirectoryChanged}
        onNavigate={onNavigate}
      />
    </section>
  );
}
