import type { ResourceActionOutput } from "@/domain/plugin";
import type { Resource, ResourceAction } from "@/domain/resource";
import { Dialog } from "@/shared/ui/dialog";
import { CoreTextEditor } from "./core-text-editor";
import { PluginOutput } from "./plugin-output";
import { actionTitle } from "./renderers/default-renderers";

export interface ActionResult {
  resource: Resource;
  action: ResourceAction;
  output: ResourceActionOutput;
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
  const textEditView =
    result?.action.id === "core.text.edit" &&
    result.action.provides === "text_edit" &&
    result.action.access === "write" &&
    result.output.view.view === "text"
      ? result.output.view
      : null;
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
      {result && textEditView ? (
        <CoreTextEditor
          resource={result.resource}
          initialText={textEditView.text}
          onSaved={onResourceChanged}
          onClose={onClose}
        />
      ) : result ? (
        <PluginOutput
          output={result.output}
          resource={result.resource}
          onResourceChanged={onResourceChanged}
        />
      ) : null}
    </Dialog>
  );
}
