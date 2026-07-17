import { LoaderCircle, TriangleAlert } from "lucide-react";

export function LoadingState({
  label = "Loading",
  compact = false,
}: {
  label?: string;
  compact?: boolean;
}) {
  return (
    <div
      className={
        compact
          ? "grid min-h-20 place-items-center text-sm text-slate-500"
          : "grid min-h-40 place-items-center text-sm text-slate-500"
      }
    >
      <span className="flex items-center gap-2">
        <LoaderCircle className="animate-spin" size={18} /> {label}
      </span>
    </div>
  );
}

export function ErrorState({ error, compact = false }: { error: unknown; compact?: boolean }) {
  return (
    <div
      className={`${compact ? "m-3 p-3" : "m-4 p-4"} flex items-start gap-2 rounded-xl border border-red-200 bg-red-50 text-sm text-red-700`}
    >
      <TriangleAlert className="mt-0.5 shrink-0" size={18} />
      <span>{error instanceof Error ? error.message : "Unexpected error"}</span>
    </div>
  );
}
