import React from "react";
import type {
  PluginActionOutput,
  PluginDiagnostic,
  Resource,
} from "../../api/contracts";
import type { PluginView } from "../host/contracts";
import { BinaryUrlView, MediaView } from "./MediaView";
import { PluginFrameView } from "./PluginFrameView";

const JsonSchemaFormView = React.lazy(() => import("./JsonSchemaFormView"));

export function PluginActionResult({
  output,
  resource,
  onResourceChanged,
  large = false,
}: {
  output: PluginActionOutput;
  resource: Resource;
  onResourceChanged?: () => void | Promise<void>;
  large?: boolean;
}) {
  return (
    <>
      <PluginDiagnostics diagnostics={output.diagnostics ?? []} />
      <PluginViewResult
        view={output.view}
        title={output.action}
        resource={resource}
        action={output.action}
        onResourceChanged={onResourceChanged}
        large={large}
      />
    </>
  );
}

function PluginDiagnostics({ diagnostics }: { diagnostics: PluginDiagnostic[] }) {
  if (!diagnostics.length) return null;
  return (
    <section className="grid gap-2 border-b border-slate-200 bg-amber-50 px-5 py-3" aria-label="Plugin diagnostics">
      {diagnostics.map((diagnostic, index) => (
        <div className="text-sm text-slate-800" key={`${diagnostic.code}:${index}`}>
          <span className="mr-2 rounded bg-white px-1.5 py-0.5 text-[11px] font-bold uppercase text-slate-600">
            {diagnostic.severity}
          </span>
          <strong>{diagnostic.code}</strong>: {diagnostic.message}
          {diagnostic.retryable ? " (retryable)" : ""}
        </div>
      ))}
    </section>
  );
}

export function PluginViewResult({
  view,
  title,
  resource,
  action,
  onResourceChanged,
  large = false,
}: {
  view: PluginView;
  title: string;
  resource: Resource;
  action: string;
  onResourceChanged?: () => void | Promise<void>;
  large?: boolean;
}) {
  if (view.view === "html") {
    return (
      <iframe
        className="block h-[72vh] max-h-180 w-full border-0 bg-white"
        sandbox=""
        title={title}
        srcDoc={htmlWithoutNetwork(view.html)}
      />
    );
  }
  if (view.view === "plugin_frame") {
    return (
      <PluginFrameView
        view={view}
        title={title}
        resource={resource}
        onResourceChanged={onResourceChanged}
        large={large}
      />
    );
  }
  if (view.view === "media") return <MediaView view={view} title={title} />;
  if (view.view === "binary_url") return <BinaryUrlView view={view} />;
  if (view.view === "text" || view.view === "markdown") {
    const text = view.view === "markdown" ? view.markdown : view.text;
    return (
      <article className="min-h-105 whitespace-pre-wrap bg-white px-7 py-6 text-[15px] leading-7 text-slate-800">
        {text}
      </article>
    );
  }
  if (view.view === "table") {
    return (
      <div className="min-h-80 overflow-auto bg-slate-50 p-5">
        <table className="w-full border-collapse bg-white text-sm">
          <thead>
            <tr>
              {view.columns.map((column) => (
                <th className="border-b border-slate-200 px-3 py-2.5 text-left text-xs font-bold uppercase text-slate-600" key={column.key}>
                  {column.label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {view.rows.map((row, rowIndex) => (
              <tr key={rowIndex}>
                {view.columns.map((column) => (
                  <td className="border-b border-slate-200 px-3 py-2.5 text-left align-top" key={column.key}>
                    {formatTableCell(row, column.key)}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    );
  }
  if (view.view === "form") {
    return (
      <PluginForm
        view={view}
        resource={resource}
        action={action}
        onResourceChanged={onResourceChanged}
      />
    );
  }
  return (
    <div className="min-h-80 overflow-auto bg-slate-50 p-5">
      <pre className="m-0 whitespace-pre-wrap font-mono text-sm leading-6 text-slate-800">
        {JSON.stringify(view.view === "json" ? view.data : view, null, 2)}
      </pre>
    </div>
  );
}

function PluginForm({
  view,
  resource,
  action,
  onResourceChanged,
}: {
  view: Extract<PluginView, { view: "form" }>;
  resource: Resource;
  action: string;
  onResourceChanged?: () => void | Promise<void>;
}) {
  const [result, setResult] = React.useState<PluginActionOutput | null>(null);

  return (
    <div className="grid gap-4 bg-white p-6">
      <React.Suspense fallback={<div className="min-h-40 p-5 text-sm text-slate-500">Loading form</div>}>
        <JsonSchemaFormView
          view={view}
          resource={resource}
          action={action}
          onExecuted={async (output, targetAction) => {
            setResult(output);
            if (targetAction.access === "read_write") await onResourceChanged?.();
          }}
        />
      </React.Suspense>
      {result && (
        <PluginActionResult
          output={result}
          resource={resource}
          onResourceChanged={onResourceChanged}
        />
      )}
    </div>
  );
}

function htmlWithoutNetwork(value: string): string {
  const policy = "default-src 'none'; img-src data:; media-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'";
  return `<meta http-equiv="Content-Security-Policy" content="${policy}">${value}`;
}

export function pluginViewTitle(view: PluginView): string | null {
  if ((view.view === "html" || view.view === "plugin_frame" || view.view === "media") && view.title) {
    return view.title;
  }
  if (view.view === "binary_url" && view.filename) return view.filename;
  return null;
}

function formatTableCell(row: unknown, key: string): string {
  const value = row && typeof row === "object"
    ? (row as Record<string, unknown>)[key]
    : undefined;
  return typeof value === "string" ? value : JSON.stringify(value ?? "");
}
