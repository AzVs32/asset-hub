import React from "react";
import { apiBase } from "../api";
import type { PluginActionOutput, PluginView } from "../types";
import { primaryButtonClass } from "./ui";

export function PluginActionResult({ output }: { output: PluginActionOutput }) {
  return <PluginViewResult view={output.view} title={output.action} resourceId={output.resource_id} action={output.action} />;
}

export function PluginViewResult({ view, title, resourceId, action }: { view: PluginView; title: string; resourceId?: string; action?: string }) {
  if (view.view === "html") {
    return <iframe className="block h-[72vh] max-h-180 w-full border-0 bg-white" sandbox="" title={title} srcDoc={htmlWithoutNetwork(view.html)} />;
  }

  if (view.view === "plugin_frame") {
    return <PluginFrame view={view} title={title} resourceId={resourceId} action={action} />;
  }

  if (view.view === "media") {
    const src =
      view.encoding === "base64" ? `data:${view.mime_type};base64,${view.data}` : mediaUrl(view.data);
    if (!src) return <InvalidUrl />;
    if (view.mime_type.startsWith("image/")) {
      return (
        <div className="flex min-h-105 items-center justify-center bg-slate-50 p-6">
          <img className="max-h-[72vh] max-w-full rounded-lg object-contain" src={src} alt={view.title || title} />
        </div>
      );
    }
    if (view.mime_type.startsWith("video/")) {
      return (
        <div className="flex min-h-105 items-center justify-center bg-slate-50 p-6">
          <video className="max-h-[72vh] max-w-full rounded-lg" src={src} title={view.title || title} controls />
        </div>
      );
    }
    if (view.mime_type === "application/pdf") {
      return <iframe className="block h-[72vh] max-h-180 w-full border-0 bg-white" title={view.title || title} src={src} />;
    }
    return (
      <div className="min-h-80 overflow-auto bg-slate-50 p-5">
        <a className={primaryButtonClass} href={src} download={view.title || title}>
          Download
        </a>
      </div>
    );
  }

  if (view.view === "binary_url") {
    const src = mediaUrl(view.url);
    if (!src) return <InvalidUrl />;
    return (
      <div className="min-h-80 overflow-auto bg-slate-50 p-5">
        <a className={primaryButtonClass} href={src} target="_blank" rel="noreferrer">
          Open
        </a>
      </div>
    );
  }

  if (view.view === "text" || view.view === "markdown") {
    const text = view.view === "markdown" ? view.markdown : view.text;
    return <article className={view.view === "markdown" ? "min-h-105 whitespace-pre-wrap bg-white px-7 py-6 font-mono text-sm leading-7 text-slate-800" : "min-h-105 whitespace-pre-wrap bg-white px-7 py-6 text-[15px] leading-7 text-slate-800"}>{text}</article>;
  }

  if (view.view === "table") {
    return (
      <div className="min-h-80 overflow-auto bg-slate-50 p-5">
        <table className="w-full border-collapse bg-white text-sm">
          <thead>
            <tr>
              {view.columns.map((column) => (
                <th className="border-b border-slate-200 px-3 py-2.5 text-left text-xs font-bold uppercase text-slate-600" key={column.key}>{column.label}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {view.rows.map((row, rowIndex) => (
              <tr key={rowIndex}>
                {view.columns.map((column) => (
                  <td className="border-b border-slate-200 px-3 py-2.5 text-left align-top" key={column.key}>{formatTableCell(row, column.key)}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    );
  }

  if (view.view === "form") {
    return <PluginForm view={view} resourceId={resourceId} action={action} />;
  }

  return (
    <div className="min-h-80 overflow-auto bg-slate-50 p-5">
      <pre className="m-0 whitespace-pre-wrap font-mono text-sm leading-6 text-slate-800">{JSON.stringify(view.view === "json" ? view.data : view, null, 2)}</pre>
    </div>
  );
}

function PluginFrame({
  view,
  title,
  resourceId,
  action,
}: {
  view: Extract<PluginView, { view: "plugin_frame" }>;
  title: string;
  resourceId?: string;
  action?: string;
}) {
  const ref = React.useRef<HTMLIFrameElement | null>(null);
  const source = pluginFrameUrl(view.url);

  React.useEffect(() => {
    async function onMessage(event: MessageEvent) {
      if (event.source !== ref.current?.contentWindow) return;
      const message = event.data;
      if (!message || message.type !== "asset-hub:execute-resource-action") return;
      if (!source || !resourceId || !action) return;
      if (message.resource_id !== resourceId || message.action !== action) return;
      if (typeof message.request_id !== "string" || message.request_id.length > 128) return;
      if (message.input !== undefined && (message.input === null || typeof message.input !== "object" || Array.isArray(message.input))) return;

      try {
        const response = await fetch(
          `${apiBase}/resources/${encodeURIComponent(message.resource_id)}/actions/${encodeURIComponent(message.action)}`,
          {
            method: "POST",
            credentials: "include",
            headers: {
              "Content-Type": "application/json",
            },
            body: JSON.stringify({
              input: message.input ?? {},
            }),
          },
        );
        const data = await response.json().catch(() => ({}));
        ref.current?.contentWindow?.postMessage(
          {
            type: "asset-hub:execute-resource-action-result",
            request_id: message.request_id,
            ok: response.ok,
            data,
            error: response.ok ? null : data?.error ?? `${response.status} ${response.statusText}`,
          },
          "*",
        );
      } catch (error) {
        ref.current?.contentWindow?.postMessage(
          {
            type: "asset-hub:execute-resource-action-result",
            request_id: message.request_id,
            ok: false,
            data: null,
            error: error instanceof Error ? error.message : "Request failed",
          },
          "*",
        );
      }
    }

    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [resourceId, action, source]);

  if (!source) {
    return <div className="min-h-40 bg-slate-50 p-5 text-sm text-red-700">Invalid plugin frame URL</div>;
  }

  return (
    <iframe
      ref={ref}
      className="block h-[72vh] max-h-180 w-full border-0 bg-white"
      sandbox="allow-scripts"
      title={view.title || title}
      src={source}
    />
  );
}

function pluginFrameUrl(value: string): string | null {
  if (!/^\/plugins\/[a-z0-9._-]+\/(?!.*(?:^|\/)\.\.(?:\/|$))/.test(value)) return null;
  return `${apiBase}${value}`;
}

function PluginForm({ view, resourceId, action }: {
  view: Extract<PluginView, { view: "form" }>;
  resourceId?: string;
  action?: string;
}) {
  const schema = view.schema && typeof view.schema === "object" ? view.schema as Record<string, unknown> : {};
  const properties = schema.properties && typeof schema.properties === "object" ? schema.properties as Record<string, Record<string, unknown>> : {};
  const initial = view.value && typeof view.value === "object" ? view.value as Record<string, unknown> : {};
  const [value, setValue] = React.useState<Record<string, unknown>>(initial);
  const [status, setStatus] = React.useState<string>("");
  const submitAction = view.submit_action || action;

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (!resourceId || !action || submitAction !== action) {
      setStatus("Form action is not allowed");
      return;
    }
    setStatus("Submitting");
    const response = await fetch(`${apiBase}/resources/${encodeURIComponent(resourceId)}/actions/${encodeURIComponent(action)}`, {
      method: "POST", credentials: "include", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ input: value }),
    });
    setStatus(response.ok ? "Submitted" : "Submit failed");
  }

  return (
    <form className="grid min-h-80 content-start gap-4 bg-white p-6" onSubmit={submit}>
      {Object.entries(properties).map(([key, field]) => {
        const label = typeof field.title === "string" ? field.title : key;
        const current = value[key];
        if (field.type === "boolean") {
          return <label className="flex items-center gap-2 text-sm" key={key}><input type="checkbox" checked={Boolean(current)} onChange={(event) => setValue((old) => ({ ...old, [key]: event.target.checked }))} />{label}</label>;
        }
        if (Array.isArray(field.enum)) {
          return <label className="grid gap-2 text-sm" key={key}><span>{label}</span><select className="rounded border border-slate-300 px-3 py-2" value={String(current ?? "")} onChange={(event) => setValue((old) => ({ ...old, [key]: event.target.value }))}>{field.enum.map((option) => <option key={String(option)} value={String(option)}>{String(option)}</option>)}</select></label>;
        }
        const numeric = field.type === "number" || field.type === "integer";
        return <label className="grid gap-2 text-sm" key={key}><span>{label}</span><input className="rounded border border-slate-300 px-3 py-2" type={numeric ? "number" : "text"} value={String(current ?? "")} onChange={(event) => setValue((old) => ({ ...old, [key]: numeric ? Number(event.target.value) : event.target.value }))} /></label>;
      })}
      <div className="flex items-center gap-3"><button className={primaryButtonClass} type="submit">Submit</button><span className="text-sm text-slate-500">{status}</span></div>
    </form>
  );
}

function mediaUrl(value: string): string {
  if (!value.startsWith("/") || value.startsWith("//")) return "";
  return `${apiBase}${value}`;
}

function htmlWithoutNetwork(value: string): string {
  const policy = "default-src 'none'; img-src data:; media-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'";
  return `<meta http-equiv="Content-Security-Policy" content="${policy}">${value}`;
}

function InvalidUrl() {
  return <div className="min-h-40 bg-slate-50 p-5 text-sm text-red-700">Invalid or external plugin URL</div>;
}

export function pluginViewTitle(view: PluginView): string | null {
  if ((view.view === "html" || view.view === "plugin_frame" || view.view === "media") && view.title) return view.title;
  if (view.view === "binary_url" && view.filename) return view.filename;
  return null;
}

function formatTableCell(row: unknown, key: string): string {
  const value = row && typeof row === "object" ? (row as Record<string, unknown>)[key] : undefined;
  return typeof value === "string" ? value : JSON.stringify(value ?? "");
}
