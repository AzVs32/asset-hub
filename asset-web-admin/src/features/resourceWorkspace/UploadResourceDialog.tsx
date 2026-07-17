import React from "react";
import { FileUp, Loader2, X } from "lucide-react";
import type { ResourceKindOption } from "../../api/contracts";
import { SelectInput, TextInput } from "../../components/forms";
import { iconButtonClass, primaryButtonClass, secondaryButtonClass } from "../../components/ui";
import { modalActionsClass, modalBackdropClass, modalClass, modalFormClass, modalHeaderClass } from "./dialogStyles";
import type { UploadDraft } from "./models";

export function UploadResourceDialog({ draft, setDraft, kinds, directories, busy, onClose, onSubmit }: {
  draft: UploadDraft; setDraft: React.Dispatch<React.SetStateAction<UploadDraft>>; kinds: ResourceKindOption[];
  directories: string[]; busy: boolean; onClose: () => void; onSubmit: (draft: UploadDraft) => Promise<unknown>;
}) {
  return <div className={modalBackdropClass}><section className={modalClass} aria-label="Upload resource">
    <header className={modalHeaderClass}><h2 className="text-xl font-bold">Upload</h2><button className={iconButtonClass} type="button" onClick={onClose}><X size={18} /></button></header>
    <form className={modalFormClass} onSubmit={(event) => { event.preventDefault(); void onSubmit(draft); }}>
      <label className="col-span-full flex min-h-20 cursor-pointer items-center gap-3 rounded-xl border-2 border-dashed border-slate-300 bg-slate-50 p-4 text-sm font-medium">
        <input className="sr-only" type="file" onChange={(e) => { const file = e.target.files?.[0] ?? null; setDraft((d) => ({ ...d, file, name: d.name || file?.name || "" })); }} />
        <FileUp size={22} /><span>{draft.file?.name ?? "Choose file"}</span>
      </label>
      <TextInput label="Name" value={draft.name} onChange={(name) => setDraft((d) => ({ ...d, name }))} />
      <SelectInput label="Kind" value={draft.kind} options={kinds} placeholder="Automatic (server detected)" onChange={(kind) => setDraft((d) => ({ ...d, kind }))} />
      <TextInput label="Directory" value={draft.directory} list="upload-directories" onChange={(directory) => setDraft((d) => ({ ...d, directory }))} />
      <datalist id="upload-directories">{directories.map((directory) => <option key={directory} value={directory} />)}</datalist>
      <TextInput label="Description" value={draft.description} onChange={(description) => setDraft((d) => ({ ...d, description }))} />
      <TextInput label="Tags" value={draft.tags} onChange={(tags) => setDraft((d) => ({ ...d, tags }))} />
      <div className={modalActionsClass}><button className={secondaryButtonClass} type="button" onClick={onClose}>Cancel</button>
        <button className={primaryButtonClass} type="submit" disabled={busy}>{busy ? <Loader2 className="animate-spin" size={18} /> : <FileUp size={18} />}Upload</button></div>
    </form>
  </section></div>;
}
