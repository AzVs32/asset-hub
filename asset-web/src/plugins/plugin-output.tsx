import { useGateway } from "@/application/ports/gateway-context";
import type { ResourceActionOutput } from "@/domain/plugin";
import type { Resource } from "@/domain/resource";
import { PluginViewHost } from "@/kernel/plugin-view-host";

export function PluginOutput({
  output,
  resource,
  onResourceChanged,
}: {
  output: ResourceActionOutput;
  resource: Resource;
  onResourceChanged?: (() => void | Promise<void>) | undefined;
}) {
  const gateway = useGateway();
  return (
    <div>
      {output.diagnostics.length ? (
        <section
          className="m-4 grid gap-2 rounded-2xl border border-amber-200 bg-amber-50 p-4 shadow-sm"
          aria-label="Plugin diagnostics"
        >
          {output.diagnostics.map((item) => (
            <p
              className="text-sm leading-6 text-amber-950"
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
