import type { Directory } from "@/domain/directory";
import { formatDate } from "@/shared/format";
import { DirectoryThumbnail } from "./asset-thumbnail";
import {
  CopyableValue,
  DetailAdvanced,
  DetailPanel,
  DetailRow,
  DetailSection,
} from "./detail-panel";

export function DirectoryDetail({ directory }: { directory: Directory }) {
  const path = formatDirectory(directory.path);
  return (
    <DetailPanel
      thumbnail={<DirectoryThumbnail size={48} />}
      title={directory.name || "Root"}
      subtitle={path}
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
        <DetailRow label="Revision">{directory.revision}</DetailRow>
      </DetailAdvanced>
    </DetailPanel>
  );
}

function formatDirectory(directory: string): string {
  return directory ? `/${directory}` : "/";
}
