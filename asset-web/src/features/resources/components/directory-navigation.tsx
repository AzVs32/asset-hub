import ChevronRightIcon from "@mui/icons-material/ChevronRight";
import { Breadcrumbs, Button, Link, Stack } from "@mui/material";
import React from "react";
import { breadcrumbs } from "@/domain/directory-path";
import type { Directory, DirectoryKind } from "@/domain/resource";
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
    <Breadcrumbs
      separator={<ChevronRightIcon fontSize="small" />}
      aria-label="Directory path"
      sx={{
        minWidth: 0,
        flex: 1,
        flexBasis: { xs: "100%", md: "auto" },
        order: { xs: 3, md: 0 },
        overflow: "hidden",
      }}
    >
      {crumbs.map((crumb) => (
        <Link
          key={crumb.path || "root"}
          component="button"
          underline="hover"
          onClick={() => onNavigate(crumb.path)}
        >
          {crumb.label}
        </Link>
      ))}
    </Breadcrumbs>
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
    <Stack
      direction="row"
      spacing={1}
      alignItems="center"
      sx={{ width: { xs: "100%", sm: "auto" }, order: { xs: 4, lg: 0 } }}
    >
      <KindSelect
        label="Directory kind"
        kinds={kinds}
        showKind
        size="small"
        value={kind}
        disabled={!canEdit || pending}
        onChange={(event) =>
          setDraft({
            directoryId: directory?.id,
            baseKind: directory?.kind,
            value: event.target.value,
          })
        }
        sx={{ minWidth: { xs: 0, sm: 220 }, flex: 1 }}
      />
      <Button
        variant="contained"
        size="small"
        disabled={!changed || pending}
        onClick={() => onKindChange(kind)}
      >
        {pending ? "Saving…" : "Apply"}
      </Button>
    </Stack>
  );
}
