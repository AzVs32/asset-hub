import { MenuItem, TextField, type TextFieldProps } from "@mui/material";

interface KindSelectItem {
  kind: string;
  label: string;
  parent: string | null;
}

interface KindTreeOption {
  item: KindSelectItem;
  prefix: string;
}

function kindTreeOptions(kinds: readonly KindSelectItem[]): KindTreeOption[] {
  const knownKinds = new Set(kinds.map((item) => item.kind));
  const children = new Map<string | null, KindSelectItem[]>();
  for (const item of kinds) {
    const parent = item.parent && knownKinds.has(item.parent) ? item.parent : null;
    const siblings = children.get(parent) ?? [];
    siblings.push(item);
    children.set(parent, siblings);
  }

  const options: KindTreeOption[] = [];
  const visited = new Set<string>();
  const visit = (item: KindSelectItem, depth: number) => {
    if (visited.has(item.kind)) return;
    visited.add(item.kind);
    const prefix = "   ".repeat(depth);
    options.push({ item, prefix });

    const nested = children.get(item.kind) ?? [];
    nested.forEach((child) => {
      visit(child, depth + 1);
    });
  };

  const roots = children.get(null) ?? [];
  roots.forEach((root) => {
    visit(root, 0);
  });
  for (const item of kinds) visit(item, 0);
  return options;
}

interface KindSelectProps extends Omit<TextFieldProps, "select" | "children"> {
  kinds: readonly KindSelectItem[];
  emptyOption?: { label: string; value?: string };
  showKind?: boolean;
  isKindDisabled?: (kind: string) => boolean;
}

export function KindSelect({
  kinds,
  emptyOption,
  showKind = false,
  isKindDisabled,
  ...props
}: KindSelectProps) {
  return (
    <TextField select fullWidth {...props}>
      {emptyOption ? (
        <MenuItem value={emptyOption.value ?? ""}>{emptyOption.label}</MenuItem>
      ) : null}
      {kindTreeOptions(kinds).map(({ item, prefix }) => (
        <MenuItem key={item.kind} value={item.kind} disabled={isKindDisabled?.(item.kind)}>
          {prefix}
          {item.label}
          {showKind ? ` · ${item.kind}` : ""}
        </MenuItem>
      ))}
    </TextField>
  );
}
