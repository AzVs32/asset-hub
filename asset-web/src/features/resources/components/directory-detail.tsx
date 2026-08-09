import { Download, Folder, Play } from "lucide-react";
import type { Directory, DirectoryAction, DirectoryKind } from "@/domain/resource";
import { usePluginKernel } from "@/kernel/plugin-kernel";
import { hostSlots } from "@/kernel/slots";
import { Button } from "@/shared/ui/button";

export function DirectoryDetail({
  directory,
  kind,
  pending,
  onAction,
}: {
  directory: Directory;
  kind: DirectoryKind | null;
  pending: boolean;
  onAction: (action: DirectoryAction) => void;
}) {
  const kernel = usePluginKernel();
  const actions = kernel.directoryActionsAt(directory, hostSlots.directoryDetail);
  return (
    <aside
      className="min-h-0 overflow-auto border-l border-slate-200 bg-slate-50/60"
      aria-label="Directory details"
    >
      <div className="grid gap-5 p-5 xl:p-6">
        <header className="flex items-start gap-4">
          <span className="grid size-11 shrink-0 place-items-center rounded-lg bg-amber-100 text-amber-700">
            <Folder size={21} />
          </span>
          <div className="min-w-0">
            <h2 className="break-words text-xl font-bold text-slate-950">
              {directory.name || "/"}
            </h2>
            <code className="mt-1 block truncate text-[11px] text-slate-400">{directory.id}</code>
          </div>
        </header>

        {actions.length ? (
          <div className="flex flex-wrap gap-2" data-plugin-slot={hostSlots.directoryDetail}>
            {actions.map((action) => (
              <Button
                key={action.id}
                variant="secondary"
                size="small"
                disabled={pending}
                title={action.description ?? action.id}
                onClick={() => onAction(action)}
              >
                {action.output.views.includes("download") ? (
                  <Download size={15} />
                ) : (
                  <Play size={15} />
                )}
                {action.label}
              </Button>
            ))}
          </div>
        ) : null}

        <dl className="grid grid-cols-2 gap-x-4 rounded-lg border border-slate-200 bg-white p-4 text-sm">
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
    <div className={`min-w-0 border-b border-slate-100 py-2 ${wide ? "col-span-2" : ""}`}>
      <dt className="text-[11px] font-semibold uppercase tracking-wide text-slate-400">{label}</dt>
      <dd className="mt-1 break-words text-slate-700">{value}</dd>
    </div>
  );
}
