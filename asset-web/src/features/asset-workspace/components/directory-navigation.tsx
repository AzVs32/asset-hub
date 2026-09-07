import CheckIcon from "@mui/icons-material/Check";
import CreateNewFolderIcon from "@mui/icons-material/CreateNewFolder";
import FolderIcon from "@mui/icons-material/Folder";
import RefreshIcon from "@mui/icons-material/Refresh";
import UploadFileIcon from "@mui/icons-material/UploadFile";
import {
  Avatar,
  Box,
  CircularProgress,
  IconButton,
  InputBase,
  Stack,
  Tooltip,
} from "@mui/material";
import React from "react";
import { useAssetWorkspaceGateway } from "@/shared/api/gateway-context";
import { formatDirectory } from "@/shared/format";

/** Current directory path and directory-level actions. */
export function DirectoryNavigation({
  path,
  onNavigate,
  onRefresh,
  onCreateFolder,
  onUpload,
}: {
  path: string;
  onNavigate: (path: string) => void;
  onRefresh: () => Promise<{ isError: boolean }>;
  onCreateFolder: () => void;
  onUpload: () => void;
}) {
  const [refreshState, setRefreshState] = React.useState<
    "idle" | "refreshing" | "success" | "error"
  >("idle");

  React.useEffect(() => {
    if (refreshState === "idle" || refreshState === "refreshing") return;
    const timeout = window.setTimeout(() => setRefreshState("idle"), 2_000);
    return () => window.clearTimeout(timeout);
  }, [refreshState]);

  async function refresh() {
    setRefreshState("refreshing");
    try {
      const result = await onRefresh();
      setRefreshState(result.isError ? "error" : "success");
    } catch {
      setRefreshState("error");
    }
  }

  return (
    <Box
      sx={{
        gridArea: "navigation",
        minWidth: 0,
        display: "flex",
        alignItems: "center",
        gap: 0.5,
      }}
    >
      <Box
        component="nav"
        aria-label="Current directory"
        sx={{
          minWidth: 0,
          flex: 1,
          bgcolor: "rgba(238, 242, 255, 0.74)",
          border: "1px solid",
          borderColor: "rgba(165, 180, 252, 0.4)",
          borderRadius: "6px",
          px: 0.75,
          py: 0.5,
          display: "flex",
          alignItems: "center",
          gap: 0.75,
        }}
      >
        <Avatar sx={{ bgcolor: "rgba(199, 210, 254, 0.7)", width: 30, height: 30 }}>
          <FolderIcon fontSize="small" />
        </Avatar>
        <DirectoryPathInput path={path} onNavigate={onNavigate} />
        <Tooltip
          title={
            refreshState === "refreshing"
              ? "Refreshing…"
              : refreshState === "success"
                ? "Refreshed"
                : refreshState === "error"
                  ? "Refresh failed"
                  : "Refresh"
          }
        >
          <span>
            <IconButton
              size="small"
              aria-label="Refresh"
              disabled={refreshState === "refreshing"}
              onClick={() => void refresh()}
            >
              {refreshState === "refreshing" ? (
                <CircularProgress size={16} />
              ) : refreshState === "success" ? (
                <CheckIcon fontSize="small" color="success" />
              ) : (
                <RefreshIcon
                  fontSize="small"
                  color={refreshState === "error" ? "error" : undefined}
                />
              )}
            </IconButton>
          </span>
        </Tooltip>
      </Box>
      <Stack direction="row" spacing={0.25} flexShrink={0}>
        <Tooltip title="New folder">
          <IconButton size="small" aria-label="New folder" onClick={onCreateFolder}>
            <CreateNewFolderIcon fontSize="small" />
          </IconButton>
        </Tooltip>
        <Tooltip title="Upload">
          <IconButton size="small" aria-label="Upload" color="primary" onClick={onUpload}>
            <UploadFileIcon fontSize="small" />
          </IconButton>
        </Tooltip>
      </Stack>
    </Box>
  );
}

function DirectoryPathInput({
  path,
  onNavigate,
}: {
  path: string;
  onNavigate: (path: string) => void;
}) {
  const gateway = useAssetWorkspaceGateway();
  const [value, setValue] = React.useState(formatDirectory(path));
  const [invalid, setInvalid] = React.useState(false);
  const [pending, setPending] = React.useState(false);

  React.useEffect(() => {
    setValue(formatDirectory(path));
    setInvalid(false);
  }, [path]);

  async function navigate() {
    const target = value.trim().replace(/^\/+|\/+$/g, "");
    setPending(true);
    setInvalid(false);
    try {
      const listing = await gateway.listDirectory({ directory: target, page: 1, limit: 1 });
      onNavigate(listing.path);
    } catch {
      setInvalid(true);
    } finally {
      setPending(false);
    }
  }

  return (
    <InputBase
      value={value}
      disabled={pending}
      onChange={(event) => {
        setValue(event.target.value);
        setInvalid(false);
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          void navigate();
        }
      }}
      inputProps={{
        "aria-label": "Directory path",
        "aria-invalid": invalid,
      }}
      sx={{
        minWidth: 0,
        flex: 1,
        px: 0.5,
        fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
        fontSize: "0.875rem",
        color: invalid ? "error.main" : "text.primary",
        "& input": {
          minWidth: 0,
          textOverflow: "ellipsis",
        },
      }}
    />
  );
}
