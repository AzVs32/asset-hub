import { X } from "lucide-react";
import { apiBase } from "../../api";
import { PluginViewResult } from "../../components/PluginViewResult";
import { cx, iconButtonClass } from "../../components/ui";
import type { Resource, ResourceReadResponse } from "../../types";
import { isImageResource } from "../../utils/resourceDrafts";
import { modalBackdropClass, modalClass, modalHeaderClass } from "./dialogStyles";

export function ResourcePreviewDialog({ reader, resource, onClose }: {
  reader?: ResourceReadResponse | null; resource?: Resource | null; onClose: () => void;
}) {
  const name = reader?.name ?? resource?.name;
  if (!name) return null;
  return <div className={modalBackdropClass}><section className={cx(modalClass, reader ? "max-w-4xl" : "max-w-5xl")} aria-label="Preview resource">
    <header className={modalHeaderClass}><div><h2 className="text-xl font-bold">{name}</h2>
      <span className="text-xs text-slate-500">{reader ? `${reader.kind} / ${reader.view.view}` : `${resource?.kind} / preview`}</span></div>
      <button className={iconButtonClass} type="button" onClick={onClose}><X size={18} /></button></header>
    {reader ? <PluginViewResult view={reader.view} title={reader.name} /> : resource && (isImageResource(resource)
      ? <div className="flex min-h-105 items-center justify-center bg-slate-50 p-6"><img className="max-h-[72vh] max-w-full rounded-lg object-contain" alt={resource.name} src={`${apiBase}/resources/${resource.id}/preview`} /></div>
      : <iframe className="block h-[72vh] w-full border-0 bg-slate-50" title={resource.name} src={`${apiBase}/resources/${resource.id}/preview`} />)}
  </section></div>;
}
