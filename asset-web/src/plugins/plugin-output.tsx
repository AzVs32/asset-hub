import { useGateway } from "@/application/ports/gateway-context";
import type { WorkspaceResource } from "@/application/workspace/workspace-scope";
import type { PluginActionOutput } from "@/domain/plugin";
import { PluginViewHost } from "@/kernel/plugin-view-host";

export function PluginOutput({
  output,
  resource,
  onResourceChanged,
}: {
  output: PluginActionOutput;
  resource: WorkspaceResource;
  onResourceChanged?: (() => void | Promise<void>) | undefined;
}) {
  const gateway = useGateway();
  return (
    <div>
      {output.diagnostics.length ? (
        <section
          className="grid gap-2 border-b border-amber-200 bg-amber-50 p-4"
          aria-label="Plugin diagnostics"
        >
          {output.diagnostics.map((item) => (
            <p
              className="text-sm text-amber-950"
              key={`${item.severity}:${item.code}:${item.message}`}
            >
              <strong className="mr-2 uppercase">{item.severity}</strong>
              {item.message}
            </p>
          ))}
        </section>
      ) : null}
      <PluginViewHost
        output={output}
        resource={resource}
        gateway={gateway}
        onResourceChanged={onResourceChanged}
      />
    </div>
  );
}
