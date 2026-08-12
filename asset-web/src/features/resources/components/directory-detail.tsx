import { Folder } from "lucide-react";
import type { Directory, DirectoryKind } from "@/domain/resource";

export function DirectoryDetail({
  directory,
  kind,
}: {
  directory: Directory;
  kind: DirectoryKind | null;
}) {
  return (
    <aside
      className="min-h-0 overflow-auto rounded-3xl border border-slate-200/80 bg-white shadow-[0_18px_50px_-30px_rgba(15,23,42,0.45)]"
      aria-label="Directory details"
    >
      <div className="grid gap-5 p-5">
        <header className="flex items-start gap-4">
          <span className="grid size-11 shrink-0 place-items-center rounded-2xl bg-gradient-to-br from-amber-100 to-orange-100 text-amber-700 shadow-sm ring-1 ring-amber-200/70">
            <Folder size={20} />
          </span>
          <div className="min-w-0 pt-0.5">
            <p className="text-[10px] font-bold uppercase tracking-[0.15em] text-slate-400">
              Folder details
            </p>
            <h2 className="mt-0.5 break-words text-lg font-bold tracking-[-0.025em] text-slate-950">
              {directory.name || "/"}
            </h2>
            <code className="mt-1 block truncate text-[11px] text-slate-400">{directory.id}</code>
          </div>
        </header>

        <dl className="grid grid-cols-2 gap-x-4 rounded-2xl border border-slate-200/80 bg-slate-50/65 p-4 text-sm">
          <Fact label="Path" value={directory.path || "/"} wide />
          <Fact label="Parent" value={directory.parentPath || "/"} wide />
          <Fact label="Kind" value={directory.kind} />
          <Fact label="Kind origin" value={kind ? `${kind.origin.kind}:${kind.origin.id}` : "-"} />
          <Fact
            label="Actions"
            value={directory.actions.map((action) => action.id).join(", ") || "-"}
            wide
          />
        </dl>
      </div>
    </aside>
  );
}

function Fact({ label, value, wide = false }: { label: string; value: string; wide?: boolean }) {
  return (
    <div className={`min-w-0 border-b border-slate-200/70 py-2.5 ${wide ? "col-span-2" : ""}`}>
      <dt className="text-[10px] font-bold uppercase tracking-[0.12em] text-slate-400">{label}</dt>
      <dd className="mt-1 break-words text-xs font-medium leading-5 text-slate-700">{value}</dd>
    </div>
  );
}
