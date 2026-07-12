import { Database } from "lucide-react";
import type React from "react";
import { ResourceDetail } from "../../components/ResourceDetail";
import type { Draft, Resource, ResourceActionDefinition, ResourceKindOption } from "../../types";

export function ResourceDetailPanel(props: {
  resource: Resource | null; draft: Draft | null;
  setDraft: React.Dispatch<React.SetStateAction<Draft | null>>;
  resourceKinds: ResourceKindOption[]; busy: boolean;
  onSave: () => void; onRead: () => void; onPreview: () => void;
  onPluginAction: (action: ResourceActionDefinition) => void;
  onDelete: () => void; onRestore: () => void;
}) {
  return <aside className="min-w-0 bg-white max-lg:border-t max-lg:border-slate-200" aria-label="Resource detail">
    {props.resource && props.draft ? <ResourceDetail
      resource={props.resource} draft={props.draft} setDraft={props.setDraft}
      resourceKinds={props.resourceKinds} busy={props.busy} onSave={props.onSave}
      onRead={props.onRead} onPreview={props.onPreview} onPluginAction={props.onPluginAction}
      onDelete={props.onDelete} onRestore={props.onRestore}
    /> : <div className="grid min-h-64 place-items-center p-8 text-slate-400 lg:min-h-screen">
      <div className="grid justify-items-center gap-3"><Database size={32} /><span className="text-sm">Select a resource</span></div>
    </div>}
  </aside>;
}
