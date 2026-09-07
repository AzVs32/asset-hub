import FolderIcon from "@mui/icons-material/Folder";
import InsertDriveFileIcon from "@mui/icons-material/InsertDriveFile";
import { Avatar } from "@mui/material";

export function ResourceThumbnail({ size = 40 }: { size?: number }) {
  return (
    <Avatar variant="rounded" sx={{ width: size, height: size }}>
      <InsertDriveFileIcon />
    </Avatar>
  );
}

export function DirectoryThumbnail({ size = 40 }: { size?: number }) {
  return (
    <Avatar
      variant="rounded"
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
