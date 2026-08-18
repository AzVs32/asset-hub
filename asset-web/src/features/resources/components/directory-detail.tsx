import FolderIcon from "@mui/icons-material/Folder";
import {
  Avatar,
  Box,
  Card,
  CardContent,
  CardHeader,
  Divider,
  Stack,
  Typography,
} from "@mui/material";
import type { Directory, DirectoryKind } from "@/domain/resource";

export function DirectoryDetail({
  directory,
  kind,
}: {
  directory: Directory;
  kind: DirectoryKind | null;
}) {
  return (
    <Card>
      <CardHeader
        avatar={
          <Avatar>
            <FolderIcon />
          </Avatar>
        }
        title={directory.name || "/"}
        subheader={directory.id}
      />
      <Divider />
      <CardContent>
        <Stack spacing={2}>
          <Fact label="Path" value={directory.path || "/"} />
          <Fact label="Parent" value={directory.parentPath || "/"} />
          <Fact label="Kind" value={directory.kind} />
          <Fact label="Kind origin" value={kind ? `${kind.origin.kind}:${kind.origin.id}` : "-"} />
          <Fact
            label="Actions"
            value={directory.actions.map((action) => action.id).join(", ") || "-"}
          />
        </Stack>
      </CardContent>
    </Card>
  );
}

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <Box>
      <Typography variant="overline" color="text.secondary">
        {label}
      </Typography>
      <Typography variant="body2" sx={{ wordBreak: "break-word" }}>
        {value}
      </Typography>
    </Box>
  );
}
