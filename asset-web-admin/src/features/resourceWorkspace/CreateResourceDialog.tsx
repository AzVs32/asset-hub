import React from "react";
import { Loader2, Plus, X } from "lucide-react";
import { SelectInput, TextInput } from "../../components/forms";
import { iconButtonClass, inputClass, primaryButtonClass, secondaryButtonClass } from "../../components/ui";
import type { Draft, ResourceKindOption, ResourceStatus } from "../../types";
import { modalActionsClass, modalBackdropClass, modalClass, modalFormClass, modalHeaderClass } from "./dialogStyles";

export function CreateResourceDialog({ draft, setDraft, kinds, busy, onClose, onSubmit }: {
  draft: Draft; setDraft: React.Dispatch<React.SetStateAction<Draft>>; kinds: ResourceKindOption[];
  busy: boolean; onClose: () => void; onSubmit: (draft: Draft) => Promise<unknown>;
}) {
  return <div className={modalBackdropClass}><section className={modalClass} aria-label="Create resource">
    <header className={modalHeaderClass}><h2 className="text-xl font-bold">New resource</h2>
      <button className={iconButtonClass} type="button" onClick={onClose}><X size={18} /></button></header>
    <form className={modalFormClass} onSubmit={(event) => { event.preventDefault(); void onSubmit(draft); }}>
      <TextInput label="Name" value={draft.name} onChange={(name) => setDraft((d) => ({ ...d, name }))} />
      <TextInput label="Directory" value={draft.directory} onChange={(directory) => setDraft((d) => ({ ...d, directory }))} />
      <SelectInput label="Kind" value={draft.kind} options={kinds} onChange={(kind) => setDraft((d) => ({ ...d, kind }))} />
      <label className="grid gap-2"><span className="text-xs font-semibold">Status</span>
        <select className={inputClass} value={draft.status} onChange={(e) => setDraft((d) => ({ ...d, status: e.target.value as ResourceStatus }))}>
          <option value="active">active</option><option value="archived">archived</option>
        </select></label>
      <TextInput label="Description" value={draft.description} onChange={(description) => setDraft((d) => ({ ...d, description }))} />
      <TextInput label="Tags" value={draft.tags} onChange={(tags) => setDraft((d) => ({ ...d, tags }))} />
      <div className={modalActionsClass}><button className={secondaryButtonClass} type="button" onClick={onClose}>Cancel</button>
        <button className={primaryButtonClass} type="submit" disabled={busy}>{busy ? <Loader2 className="animate-spin" size={18} /> : <Plus size={18} />}Create</button></div>
    </form>
  </section></div>;
}
