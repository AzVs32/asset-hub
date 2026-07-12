import { X } from "lucide-react";
import { PluginActionResult, pluginViewTitle } from "../../components/PluginViewResult";
import { cx, iconButtonClass } from "../../components/ui";
import type { PluginActionOutput } from "../../types";
import { modalBackdropClass, modalClass, modalHeaderClass } from "./dialogStyles";

export function PluginActionPanel({ output, onClose }: { output: PluginActionOutput | null; onClose: () => void }) {
  if (!output) return null;
  return <div className={modalBackdropClass}><section className={cx(modalClass, "max-w-4xl")} aria-label="Action result">
    <header className={modalHeaderClass}><div><h2 className="text-xl font-bold">{pluginViewTitle(output.view) || output.action}</h2>
      <span className="text-xs text-slate-500">{output.action} / {output.view.view}</span></div>
      <button className={iconButtonClass} type="button" onClick={onClose}><X size={18} /></button></header>
    <PluginActionResult output={output} />
  </section></div>;
}
