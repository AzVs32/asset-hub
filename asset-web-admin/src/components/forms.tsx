import type { ResourceKindOption } from "../types";
import { kindOptionHint, kindOptionLabel } from "../utils/resourceDrafts";

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
    <label className="field">
      <span>{label}</span>
      <input value={value} list={list} onChange={(event) => onChange(event.target.value)} />
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
    <label className="field">
      <span>{label}</span>
      <select value={value} onChange={(event) => onChange(event.target.value)}>
        {options.map((option) => (
          <option key={option.kind} value={option.kind}>
            {kindOptionLabel(option)}
          </option>
        ))}
      </select>
      {value && <small>{kindOptionHint(options.find((option) => option.kind === value))}</small>}
    </label>
  );
}

export function Fact({ label, value }: { label: string; value: string }) {
  return (
    <div className="fact">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}



