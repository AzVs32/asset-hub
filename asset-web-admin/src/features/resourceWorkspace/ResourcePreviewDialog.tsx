import { ExternalLink, X } from "lucide-react";
import { apiBase } from "../../api";
import { cx, iconButtonClass } from "../../components/ui";
import { CoreVideoView } from "../../plugins/core";
import { PluginViewResult } from "../../plugins/views";
import type { Resource, ResourceReadResponse } from "../../types";
import { isImageResource, isVideoResource } from "../../utils/resourceDrafts";
import { pluginStandaloneUrl } from "../StandalonePluginView";
import { modalBackdropClass, modalClass, modalHeaderClass } from "./dialogStyles";

export function ResourcePreviewDialog({ reader, resource, onClose }: {
  reader?: ResourceReadResponse | null; resource?: Resource | null; onClose: () => void;
}) {
  const name = reader?.name ?? resource?.name;
  if (!name) return null;
  const large = reader?.view.view === "plugin_frame";
  const frameUrl = reader?.view.view === "plugin_frame" ? pluginStandaloneUrl(reader.id, "read") : null;
  return <div className={modalBackdropClass}><section className={cx(modalClass, large ? "h-[calc(100vh-2rem)] max-w-[calc(100vw-2rem)] overflow-hidden xl:max-w-7xl" : reader ? "max-w-4xl" : "max-w-5xl")} aria-label="Preview resource">
    <header className={modalHeaderClass}><div><h2 className="text-xl font-bold">{name}</h2>
      <span className="text-xs text-slate-500">{reader ? `${reader.kind} / ${reader.view.view}` : `${resource?.kind} / preview`}</span></div>
      <div className="flex items-center gap-2">
        {frameUrl && <a className={iconButtonClass} href={frameUrl} target="_blank" rel="noreferrer" title="Open in new tab"><ExternalLink size={18} /></a>}
        <button className={iconButtonClass} type="button" onClick={onClose}><X size={18} /></button>
      </div></header>
    {reader ? <PluginViewResult view={reader.view} title={reader.name} resourceId={reader.id} action="read" large={large} /> : resource && (isImageResource(resource)
      ? <div className="flex min-h-105 items-center justify-center bg-slate-50 p-6"><img className="max-h-[72vh] max-w-full rounded-lg object-contain" alt={resource.name} src={`${apiBase}/resources/${resource.id}/preview`} /></div>
      : isVideoResource(resource)
        ? <CoreVideoView src={`${apiBase}/resources/${resource.id}/preview`} title={resource.name} />
        : <iframe className="block h-[72vh] w-full border-0 bg-slate-50" title={resource.name} src={`${apiBase}/resources/${resource.id}/preview`} />)}
  </section></div>;
}
