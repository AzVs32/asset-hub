import { ChevronRight } from "lucide-react";
import React from "react";
import { breadcrumbs } from "@/domain/directory-path";
import type { Directory, DirectoryKind } from "@/domain/resource";
import { Button } from "@/shared/ui/button";
import { KindSelect } from "./kind-select";

/** Host-owned path navigation rendered in the primary Asset Hub header. */
export function DirectoryBreadcrumbs({
  path,
  onNavigate,
}: {
  path: string;
  onNavigate: (path: string) => void;
}) {
  const crumbs = breadcrumbs(path);
  return (
    <nav
      className="flex min-w-0 flex-1 items-center gap-1 overflow-hidden border-l border-slate-200 pl-5 text-sm"
      aria-label="Directory path"
    >
      {crumbs.map((crumb, index) => (
        <React.Fragment key={crumb.path || "root"}>
          {index ? <ChevronRight className="shrink-0 text-slate-300" size={15} /> : null}
          <button
            className="max-w-48 truncate rounded-lg px-2 py-1.5 font-semibold text-slate-500 transition hover:bg-indigo-50 hover:text-indigo-700"
            type="button"
            onClick={() => onNavigate(crumb.path)}
          >
            {crumb.label}
          </button>
        </React.Fragment>
      ))}
    </nav>
  );
}

/** Host-owned Directory kind editor shared by every workspace implementation. */
export function DirectoryKindEditor({
  directory,
  kinds,
  pending,
  onKindChange,
}: {
  directory: Directory | undefined;
  kinds: readonly DirectoryKind[];
  pending: boolean;
  onKindChange: (kind: string) => void;
}) {
  const kindSelectId = React.useId();
  const [draft, setDraft] = React.useState({
    directoryId: directory?.id,
    baseKind: directory?.kind,
    value: directory?.kind ?? "",
  });
  const kind =
    draft.directoryId === directory?.id && draft.baseKind === directory?.kind
      ? draft.value
      : (directory?.kind ?? "");
  const canEdit = Boolean(directory?.parentId) && kinds.length > 0;
  const changed = canEdit && Boolean(kind) && kind !== directory?.kind;

  return (
    <div className="flex shrink-0 items-end gap-2 border-l border-slate-200 pl-5">
      <div className="flex min-w-64 items-end gap-2">
        <label
          htmlFor={kindSelectId}
          className="grid min-w-0 flex-1 gap-1 text-[10px] font-bold uppercase tracking-[0.14em] text-slate-400"
        >
          Directory kind
          <KindSelect
            aria-label="Directory kind"
            id={kindSelectId}
            kinds={kinds}
            showKind
            value={kind}
            className="min-h-9 border-slate-200 bg-slate-50 py-1.5 text-xs font-semibold text-slate-700 hover:border-slate-300 focus:border-indigo-400 focus:bg-white focus:ring-indigo-100 disabled:bg-slate-100 disabled:text-slate-400"
            disabled={!canEdit || pending}
            onChange={(event) =>
              setDraft({
                directoryId: directory?.id,
                baseKind: directory?.kind,
                value: event.target.value,
              })
            }
          />
        </label>
        <Button
          variant="secondary"
          size="small"
          className="border-indigo-100 bg-indigo-50 text-indigo-700 hover:border-indigo-200 hover:bg-indigo-100"
          disabled={!changed || pending}
          onClick={() => onKindChange(kind)}
        >
          {pending ? "Saving…" : "Apply"}
        </Button>
      </div>
    </div>
  );
}
