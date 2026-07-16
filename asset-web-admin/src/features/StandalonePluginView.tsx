import React from "react";
import { ArrowLeft, Loader2, TriangleAlert } from "lucide-react";
import { request } from "../api";
import { iconButtonClass } from "../components/ui";
import { PluginActionResult, pluginViewTitle } from "../plugins/views";
import type { PluginActionOutput, Resource } from "../types";

export type StandalonePluginTarget = {
  resourceId: string;
  action: string;
};

const resourceParam = "plugin_resource";
const actionParam = "plugin_action";

export function pluginStandaloneUrl(resourceId: string, action: string): string | null {
  if (!isStandaloneValue(resourceId) || !isStandaloneValue(action)) return null;
  const url = new URL(window.location.href);
  url.search = new URLSearchParams({
    [resourceParam]: resourceId,
    [actionParam]: action,
  }).toString();
  url.hash = "";
  return url.toString();
}

export function readStandalonePluginTarget(): StandalonePluginTarget | null {
  const params = new URLSearchParams(window.location.search);
  const resourceId = params.get(resourceParam);
  const action = params.get(actionParam);
  if (!resourceId || !action || !isStandaloneValue(resourceId) || !isStandaloneValue(action)) {
    return null;
  }
  return { resourceId, action };
}

export function StandalonePluginView({ target }: { target: StandalonePluginTarget }) {
  const [output, setOutput] = React.useState<PluginActionOutput | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const homeUrl = React.useMemo(() => {
    const url = new URL(window.location.href);
    url.search = "";
    url.hash = "";
    return url.toString();
  }, []);

  React.useEffect(() => {
    let active = true;
    setOutput(null);
    setError(null);
    request<Resource>(`/resources/${encodeURIComponent(target.resourceId)}`).then((resource) => {
      const action = resource.actions.available_actions.find((candidate) => (
        candidate.id === target.action
        && candidate.access === "read_only"
        && candidate.output.view.includes("plugin_frame")
      ));
      if (!action) throw new Error("Plugin view action is unavailable");
      return request<PluginActionOutput>(
        `/resources/${encodeURIComponent(target.resourceId)}/actions/${encodeURIComponent(target.action)}`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ input: {} }),
        },
      );
    }).then((result) => {
      if (!active) return;
      if (result.resource_id !== target.resourceId || result.action !== target.action) {
        throw new Error("Plugin action returned mismatched context");
      }
      setOutput(result);
    }).catch((reason: unknown) => {
      if (active) setError(reason instanceof Error ? reason.message : "Unable to open plugin view");
    });
    return () => {
      active = false;
    };
  }, [target.action, target.resourceId]);

  React.useEffect(() => {
    document.title = output ? pluginViewTitle(output.view) || output.action : "Asset Hub";
  }, [output]);

  return (
    <main className="min-h-screen bg-slate-100">
      <header className="flex min-h-16 items-center gap-3 border-b border-slate-200 bg-white px-4 sm:px-6">
        <a className={iconButtonClass} href={homeUrl} title="Back to Asset Hub" aria-label="Back to Asset Hub">
          <ArrowLeft size={18} />
        </a>
        <div className="min-w-0">
          <h1 className="truncate text-base font-bold text-slate-900">
            {output ? pluginViewTitle(output.view) || output.action : "Opening plugin view"}
          </h1>
          <p className="truncate text-xs text-slate-500">{target.action}</p>
        </div>
      </header>
      {output ? (
        <PluginActionResult output={output} large />
      ) : error ? (
        <div className="grid min-h-[calc(100vh-4rem)] place-content-center justify-items-center gap-3 p-6 text-center text-red-700">
          <TriangleAlert size={28} />
          <p className="max-w-xl text-sm">{error}</p>
        </div>
      ) : (
        <div className="grid min-h-[calc(100vh-4rem)] place-content-center text-slate-500">
          <Loader2 className="animate-spin" size={28} aria-label="Opening plugin view" />
        </div>
      )}
    </main>
  );
}

function isStandaloneValue(value: string): boolean {
  return value.length <= 128 && /^[a-zA-Z0-9][a-zA-Z0-9._:-]*$/.test(value);
}
