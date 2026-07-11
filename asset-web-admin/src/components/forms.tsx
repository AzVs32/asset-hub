import type { ResourceKindOption } from "../types";
import { kindOptionHint, kindOptionLabel } from "../utils/resourceDrafts";
import { inputClass } from "./ui";

export function TextInput({
  label,
  value,
  onChange,
  list,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  list?: string;
}) {
  return (
    <label className="grid min-w-0 gap-2">
      <span className="text-xs font-semibold text-slate-600">{label}</span>
      <input className={inputClass} value={value} list={list} onChange={(event) => onChange(event.target.value)} />
    </label>
  );
}

export function SelectInput({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: ResourceKindOption[];
  onChange: (value: string) => void;
}) {
  return (
    <label className="grid min-w-0 gap-2">
      <span className="text-xs font-semibold text-slate-600">{label}</span>
      <select className={inputClass} value={value} onChange={(event) => onChange(event.target.value)}>
        {options.map((option) => (
          <option key={option.kind} value={option.kind}>
            {kindOptionLabel(option)}
          </option>
        ))}
      </select>
      {value && <small className="break-words text-xs leading-relaxed text-slate-500">{kindOptionHint(options.find((option) => option.kind === value))}</small>}
    </label>
  );
}

export function Fact({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 border-t border-slate-200 py-3">
      <span className="text-xs font-medium text-slate-500">{label}</span>
      <strong className="mt-1 block break-words text-sm font-semibold text-slate-800">{value}</strong>
    </div>
  );
}

