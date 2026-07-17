import React from "react";
import { apiBase } from "../../api";
import type { Resource } from "../../api/contracts";
import {
  executeResourceAction,
  findAvailableAction,
} from "../host/actions";
import type { PluginView } from "../host/contracts";
import {
  parseExecuteActionMessage,
  pluginFrameProtocolVersion,
} from "../host/frameProtocol";

export function PluginFrameView({
  view,
  title,
  resource,
  onResourceChanged,
  large = false,
}: {
  view: Extract<PluginView, { view: "plugin_frame" }>;
  title: string;
  resource: Resource;
  onResourceChanged?: () => void | Promise<void>;
  large?: boolean;
}) {
  const ref = React.useRef<HTMLIFrameElement | null>(null);
  const source = pluginFrameUrl(view.url);

  React.useEffect(() => {
    async function onMessage(event: MessageEvent) {
      if (event.source !== ref.current?.contentWindow) return;
      const message = parseExecuteActionMessage(event.data);
      if (!message || !source || message.resource_id !== resource.id) return;
      const targetAction = findAvailableAction(resource, message.action);
      if (!targetAction) {
        postResult(ref.current, message.request_id, {
          ok: false,
          data: null,
          error: `Action ${message.action} is not available for this resource`,
        });
        return;
      }

      try {
        const data = await executeResourceAction(resource, targetAction.id, message.input ?? {});
        postResult(ref.current, message.request_id, { ok: true, data, error: null });
        if (targetAction.access === "read_write") await onResourceChanged?.();
      } catch (error) {
        postResult(ref.current, message.request_id, {
          ok: false,
          data: null,
          error: error instanceof Error ? error.message : "Request failed",
        });
      }
    }

    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [onResourceChanged, resource, source]);

  if (!source) {
    return <div className="min-h-40 bg-slate-50 p-5 text-sm text-red-700">Invalid plugin frame URL</div>;
  }
  return (
    <iframe
      ref={ref}
      className={large
        ? "block h-[calc(100vh-8.5rem)] min-h-120 w-full border-0 bg-white"
        : "block h-[72vh] max-h-180 w-full border-0 bg-white"}
      sandbox="allow-scripts"
      title={view.title || title}
      src={source}
    />
  );
}

export function pluginFrameUrl(value: string): string | null {
  if (!/^\/plugins\/[a-z0-9._-]+\/(?!.*(?:^|\/)\.\.(?:\/|$))/.test(value)) return null;
  return `${apiBase}${value}`;
}

function postResult(
  frame: HTMLIFrameElement | null,
  requestId: string,
  result: { ok: boolean; data: unknown; error: string | null },
) {
  frame?.contentWindow?.postMessage({
    type: "asset-hub:execute-resource-action-result",
    version: pluginFrameProtocolVersion,
    request_id: requestId,
    ...result,
  }, "*");
}

