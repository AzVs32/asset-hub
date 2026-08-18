import { Dialog, DialogContent, DialogTitle, Typography } from "@mui/material";
import type { ResourceActionOutput } from "@/domain/plugin";
import type { Resource, ResourceAction } from "@/domain/resource";
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
  const description = result?.output.view
    ? `${result.action.id} · ${result.output.view.view}`
    : undefined;
  return (
    <Dialog open={Boolean(result)} fullWidth maxWidth="lg" onClose={onClose}>
      <DialogTitle>
        {result ? actionTitle(result.action, result.output) : "Plugin output"}
      </DialogTitle>
      <DialogContent>
        {description ? (
          <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 2 }}>
            {description}
          </Typography>
        ) : null}
        {result ? (
          <PluginOutput
            output={result.output}
            resource={result.resource}
            onResourceChanged={onResourceChanged}
          />
        ) : null}
      </DialogContent>
    </Dialog>
  );
}
