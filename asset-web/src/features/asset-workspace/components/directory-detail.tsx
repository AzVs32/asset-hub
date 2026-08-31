import { Chip } from "@mui/material";
import type { Directory, DirectoryKind } from "@/domain/directory";
import { formatDate } from "@/shared/format";
import { DirectoryThumbnail } from "./asset-thumbnail";
import {
  CopyableValue,
  DetailAdvanced,
  DetailPanel,
  DetailRow,
  DetailSection,
  DetailValueList,
} from "./detail-panel";

export function DirectoryDetail({
  directory,
  kind,
}: {
  directory: Directory;
  kind: DirectoryKind | null;
}) {
  const path = formatDirectory(directory.path);
  return (
    <DetailPanel
      thumbnail={<DirectoryThumbnail directory={directory} size={48} />}
      title={directory.name || "Root"}
      subtitle={path}
      badges={<Chip label={kind?.label ?? directory.kind} size="small" variant="outlined" />}
    >
      <DetailSection title="General">
        <DetailRow label="Created">{formatDate(directory.createdAt)}</DetailRow>
        <DetailRow label="Updated">{formatDate(directory.updatedAt)}</DetailRow>
      </DetailSection>
      <DetailAdvanced>
        <DetailRow label="Directory ID">
          <CopyableValue value={directory.id} />
        </DetailRow>
        <DetailRow label="Parent ID">
          {directory.parentId ? <CopyableValue value={directory.parentId} /> : "—"}
        </DetailRow>
        <DetailRow label="Kind ID">
          <CopyableValue value={directory.kind} />
        </DetailRow>
        <DetailRow label="Kind origin">
          {kind ? `${kind.origin.kind}:${kind.origin.id}` : "—"}
        </DetailRow>
        <DetailRow label="Revision">{directory.revision}</DetailRow>
        <DetailRow label="Actions">
          <DetailValueList values={directory.actions.map((action) => action.id)} />
        </DetailRow>
      </DetailAdvanced>
    </DetailPanel>
  );
}

function formatDirectory(directory: string): string {
  return directory ? `/${directory}` : "/";
}
