import { apiBase } from "../api";
import type { PluginActionOutput, PluginView } from "../types";

export function PluginActionResult({ output }: { output: PluginActionOutput }) {
  return <PluginViewResult view={output.view} title={output.action} />;
}

export function PluginViewResult({ view, title }: { view: PluginView; title: string }) {
  if (view.view === "html") {
    return <iframe className="plugin-html-frame" sandbox="allow-scripts" title={title} srcDoc={view.html} />;
  }

  if (view.view === "media") {
    const src =
      view.encoding === "base64" ? `data:${view.mime_type};base64,${view.data}` : mediaUrl(view.data);
    if (view.mime_type.startsWith("image/")) {
      return (
        <div className="plugin-media">
          <img src={src} alt={view.title || title} />
        </div>
      );
    }
    if (view.mime_type.startsWith("video/")) {
      return (
        <div className="plugin-media">
          <video src={src} title={view.title || title} controls />
        </div>
      );
    }
    if (view.mime_type === "application/pdf") {
      return <iframe className="plugin-html-frame" title={view.title || title} src={src} />;
    }
    return (
      <div className="plugin-result">
        <a className="primary-button" href={src} download={view.title || title}>
          Download
        </a>
      </div>
    );
  }

  if (view.view === "binary_url") {
    return (
      <div className="plugin-result">
        <a className="primary-button" href={view.url} target="_blank" rel="noreferrer">
          Open
        </a>
      </div>
    );
  }

  if (view.view === "text" || view.view === "markdown") {
    const text = view.view === "markdown" ? view.markdown : view.text;
    return <article className={view.view === "markdown" ? "plugin-markdown" : "reader-content"}>{text}</article>;
  }

  if (view.view === "table") {
    return (
      <div className="plugin-result plugin-table-wrap">
        <table className="plugin-table">
          <thead>
            <tr>
              {view.columns.map((column) => (
                <th key={column.key}>{column.label}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {view.rows.map((row, rowIndex) => (
              <tr key={rowIndex}>
                {view.columns.map((column) => (
                  <td key={column.key}>{formatTableCell(row, column.key)}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    );
  }

  return (
    <div className="plugin-result">
      <pre>{JSON.stringify(view.view === "json" ? view.data : view, null, 2)}</pre>
    </div>
  );
}

function mediaUrl(value: string): string {
  if (/^https?:\/\//i.test(value)) {
    return value;
  }
  return `${apiBase}${value.startsWith("/") ? value : `/${value}`}`;
}

export function pluginViewTitle(view: PluginView): string | null {
  if ((view.view === "html" || view.view === "media") && view.title) return view.title;
  if (view.view === "binary_url" && view.filename) return view.filename;
  return null;
}

function formatTableCell(row: unknown, key: string): string {
  const value = row && typeof row === "object" ? (row as Record<string, unknown>)[key] : undefined;
  return typeof value === "string" ? value : JSON.stringify(value ?? "");
}



