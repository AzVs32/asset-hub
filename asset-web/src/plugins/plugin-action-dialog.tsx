import CloseIcon from "@mui/icons-material/Close";
import { Dialog, DialogContent, DialogTitle, IconButton, Typography } from "@mui/material";
import type { ResourceActionOutput } from "@/domain/plugin";
import type { Resource, ResourceAction } from "@/domain/resource";
import type { ResourceChangedHandler } from "@/kernel/plugin-kernel";
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
  onResourceChanged: ResourceChangedHandler;
}) {
  const description = result?.output.view
    ? `${result.action.id} · ${result.output.view.type}`
    : undefined;
  return (
    <Dialog
      open={Boolean(result)}
      fullWidth
      maxWidth="lg"
      onClose={(_, reason) => {
        if (reason !== "backdropClick") onClose();
      }}
    >
      <DialogTitle sx={{ position: "relative", pr: 7 }}>
        {result ? actionTitle(result.action, result.output) : "Plugin output"}
        <IconButton
          aria-label="Close plugin action"
          onClick={onClose}
          sx={{ position: "absolute", top: 8, right: 8 }}
        >
          <CloseIcon />
        </IconButton>
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
