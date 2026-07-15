import { ExternalLink, X } from "lucide-react";
import { cx, iconButtonClass } from "../../components/ui";
import { PluginActionResult, pluginFrameUrl, pluginViewTitle } from "../../plugins/views";
import type { PluginActionOutput } from "../../types";
import { modalBackdropClass, modalClass, modalHeaderClass } from "./dialogStyles";

export function PluginActionPanel({ output, onClose }: { output: PluginActionOutput | null; onClose: () => void }) {
  if (!output) return null;
  const frameUrl = output.view.view === "plugin_frame" ? pluginFrameUrl(output.view.url) : null;
  const large = output.view.view === "plugin_frame";
  return <div className={modalBackdropClass}><section className={cx(modalClass, large && "h-[calc(100vh-2rem)] max-w-[calc(100vw-2rem)] overflow-hidden xl:max-w-7xl")} aria-label="Action result">
    <header className={modalHeaderClass}><div><h2 className="text-xl font-bold">{pluginViewTitle(output.view) || output.action}</h2>
      <span className="text-xs text-slate-500">{output.action} / {output.view.view}</span></div>
      <div className="flex items-center gap-2">
        {frameUrl && <a className={iconButtonClass} href={frameUrl} target="_blank" rel="noreferrer" title="Open in new tab"><ExternalLink size={18} /></a>}
        <button className={iconButtonClass} type="button" onClick={onClose}><X size={18} /></button>
      </div></header>
    <PluginActionResult output={output} large={large} />
  </section></div>;
}
