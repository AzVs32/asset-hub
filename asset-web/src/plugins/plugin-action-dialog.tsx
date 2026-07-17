import type { PluginActionOutput } from "@/domain/plugin";
import type { Resource, ResourceAction } from "@/domain/resource";
import { Dialog } from "@/shared/ui/dialog";
import { PluginOutput } from "./plugin-output";
import { actionTitle } from "./renderers/default-renderers";

export interface ActionResult {
  resource: Resource;
  action: ResourceAction;
  output: PluginActionOutput;
}

export function PluginActionDialog({
  result,
  onClose,
  onResourceChanged,
}: {
  result: ActionResult | null;
  onClose: () => void;
  onResourceChanged: () => void | Promise<void>;
}) {
  return (
    <Dialog
      open={Boolean(result)}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={result ? actionTitle(result.action, result.output) : "Plugin output"}
      description={result ? `${result.action.id} · ${result.output.view.view}` : undefined}
      className="max-w-5xl"
    >
      {result ? (
        <PluginOutput
          output={result.output}
          resource={result.resource}
          onResourceChanged={onResourceChanged}
        />
      ) : null}
    </Dialog>
  );
}
