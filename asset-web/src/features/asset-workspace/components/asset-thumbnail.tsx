import FolderIcon from "@mui/icons-material/Folder";
import InsertDriveFileIcon from "@mui/icons-material/InsertDriveFile";
import { Avatar } from "@mui/material";
import { useQuery } from "@tanstack/react-query";
import type { Directory } from "@/domain/directory";
import type { PluginView } from "@/domain/plugin";
import type { Resource } from "@/domain/resource";
import { usePluginKernel } from "@/kernel/plugin-kernel";
import { usePluginHostGateway } from "@/shared/api/gateway-context";

export function ResourceThumbnail({ resource, size = 40 }: { resource: Resource; size?: number }) {
  const gateway = usePluginHostGateway();
  const kernel = usePluginKernel();
  const action = kernel.thumbnailAction(resource);
  const result = useQuery({
    queryKey: ["resource-thumbnail", resource.id, resource.revision, action?.id],
    queryFn: () => {
      if (!action) throw new Error("Resource thumbnail action is unavailable");
      return gateway.executeResourceAction(resource, action.id);
    },
    enabled: Boolean(action),
    retry: false,
    staleTime: 5 * 60_000,
  });
  const image = result.data
    ? thumbnailImage(result.data.view, gateway.assetUrl.bind(gateway))
    : null;

  return (
    <Avatar
      variant="rounded"
      src={image ?? undefined}
      alt={image ? `${resource.name} thumbnail` : undefined}
      sx={{ width: size, height: size }}
    >
      <InsertDriveFileIcon />
    </Avatar>
  );
}

export function DirectoryThumbnail({
  directory,
  size = 40,
}: {
  directory: Directory;
  size?: number;
}) {
  const gateway = usePluginHostGateway();
  const kernel = usePluginKernel();
  const action = kernel.directoryThumbnailAction(directory);
  const result = useQuery({
    queryKey: ["directory-thumbnail", directory.id, directory.revision, action?.id],
    queryFn: () => {
      if (!action) throw new Error("Directory thumbnail action is unavailable");
      return gateway.executeDirectoryAction(directory, action.id);
    },
    enabled: Boolean(action),
    retry: false,
    staleTime: 5 * 60_000,
  });
  const image = result.data
    ? thumbnailImage(result.data.view, gateway.assetUrl.bind(gateway))
    : null;

  return (
    <Avatar
      variant="rounded"
      src={image ?? undefined}
      alt={image ? `${directory.name || "Root"} thumbnail` : undefined}
      sx={{
        width: size,
        height: size,
        color: "warning.dark",
        background: "linear-gradient(145deg, #fffbeb, #ffedd5)",
      }}
    >
      <FolderIcon />
    </Avatar>
  );
}

function thumbnailImage(
  view: PluginView | null,
  resolveUrl: (url: string) => string | null,
): string | null {
  if (view?.type !== "media" || !view.mime_type.startsWith("image/")) return null;
  return view.encoding === "base64"
    ? `data:${view.mime_type};base64,${view.data}`
    : resolveUrl(view.data);
}
