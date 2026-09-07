import ChevronRightIcon from "@mui/icons-material/ChevronRight";
import DeleteIcon from "@mui/icons-material/DeleteOutline";
import FolderIcon from "@mui/icons-material/Folder";
import MoreVertIcon from "@mui/icons-material/MoreVert";
import {
  Alert,
  Avatar,
  Box,
  Card,
  CardActions,
  CircularProgress,
  Divider,
  IconButton,
  List,
  ListItem,
  ListItemAvatar,
  ListItemButton,
  ListItemText,
  Menu,
  MenuItem,
  Pagination,
  Stack,
  Tooltip,
  Typography,
} from "@mui/material";
import React from "react";
import type { Directory, DirectoryListing, DirectoryListingQuery } from "@/domain/directory";
import { parentDirectory } from "@/domain/directory-path";
import type { Resource } from "@/domain/resource";
import { formatBytes, formatDate } from "@/shared/format";
import { DirectoryThumbnail, ResourceThumbnail } from "./asset-thumbnail";

export function WorkspaceList({
  listing,
  filters,
  selectedId,
  selectedDirectoryId,
  loading,
  mutating,
  error,
  onFilters,
  onOpenDirectory,
  onSelect,
  onSelectDirectory,
  onDownloadResource,
  onDeleteResource,
  onDownloadDirectory,
  onDeleteDirectory,
}: {
  listing: DirectoryListing | undefined;
  filters: DirectoryListingQuery;
  selectedId: string | null;
  selectedDirectoryId: string | null;
  loading: boolean;
  mutating: boolean;
  error: unknown;
  onFilters: (page: number) => void;
  onOpenDirectory: (path: string) => void;
  onSelect: (resource: Resource) => void;
  onSelectDirectory: (directory: Directory) => void;
  onDownloadResource: (resource: Resource) => void;
  onDeleteResource: (resource: Resource) => void;
  onDownloadDirectory: (directory: Directory) => void;
  onDeleteDirectory: (directory: Directory) => void;
}) {
  const total = listing?.resources.total ?? 0;
  const totalPages = Math.max(1, Math.ceil(total / filters.limit));
  const parent = parentDirectory(filters.directory);
  const blocked = mutating || loading || Boolean(error);

  return (
    <Card sx={{ display: "flex", flexDirection: "column", minHeight: 0, minWidth: 0 }}>
      <Box sx={{ flex: 1, minHeight: 0, overflow: "auto" }}>
        {error ? (
          <Alert severity="error" sx={{ m: 2 }}>
            {error instanceof Error ? error.message : "Unexpected error"}
          </Alert>
        ) : null}
        {loading && !listing ? (
          <Box sx={{ display: "flex", justifyContent: "center", p: 4 }}>
            <CircularProgress />
          </Box>
        ) : null}
        <List disablePadding>
          {parent !== null ? <FolderRow name=".." onOpen={() => onOpenDirectory(parent)} /> : null}
          {listing?.folders.map((folder) => (
            <FolderRow
              key={folder.id}
              name={folder.name}
              disabled={blocked}
              directory={folder}
              selected={folder.id === selectedDirectoryId}
              onSelect={() => onSelectDirectory(folder)}
              onOpen={() => onOpenDirectory(folder.path)}
              onDownload={() => onDownloadDirectory(folder)}
              onDelete={() => onDeleteDirectory(folder)}
            />
          ))}
          {listing?.resources.items.map((resource) => (
            <ResourceRow
              key={resource.id}
              disabled={blocked}
              resource={resource}
              selected={resource.id === selectedId}
              onSelect={() => onSelect(resource)}
              onDownload={() => onDownloadResource(resource)}
              onDelete={() => onDeleteResource(resource)}
            />
          ))}
        </List>
        {!loading &&
        !error &&
        listing &&
        !listing.folders.length &&
        listing.resources.total === 0 ? (
          <Box sx={{ display: "grid", placeItems: "center", minHeight: 320 }}>
            <Stack alignItems="center" spacing={1.5}>
              <Avatar sx={{ width: 56, height: 56 }}>
                <FolderIcon />
              </Avatar>
              <Typography variant="body2" color="text.secondary">
                This folder is empty
              </Typography>
            </Stack>
          </Box>
        ) : null}
      </Box>
      <Divider />
      <CardActions sx={{ justifyContent: "center" }}>
        <Pagination
          count={totalPages}
          page={Math.min(filters.page, totalPages)}
          disabled={loading}
          size="small"
          onChange={(_event, page) => onFilters(page)}
        />
      </CardActions>
    </Card>
  );
}

function FolderRow({
  name,
  directory,
  disabled = false,
  selected = false,
  onSelect,
  onOpen,
  onDownload,
  onDelete,
}: {
  name: string;
  directory?: Directory;
  disabled?: boolean;
  selected?: boolean;
  onSelect?: () => void;
  onOpen?: () => void;
  onDownload?: () => void;
  onDelete?: () => void;
}) {
  return (
    <ListItem
      disablePadding
      secondaryAction={
        directory && onDownload && onDelete ? (
          <Stack direction="row">
            <Tooltip title="Open folder">
              <span>
                <IconButton
                  size="small"
                  disabled={disabled}
                  aria-label={`Open ${name}`}
                  onClick={onOpen}
                >
                  <ChevronRightIcon />
                </IconButton>
              </span>
            </Tooltip>
            <RowActions
              disabled={disabled}
              items={[
                { id: "download", label: "Download", onSelect: onDownload },
                { id: "delete", label: "Delete", destructive: true, onSelect: onDelete },
              ]}
            />
          </Stack>
        ) : null
      }
    >
      <ListItemButton
        disabled={disabled}
        selected={selected}
        aria-pressed={directory ? selected : undefined}
        sx={{ pr: directory ? 10 : 2 }}
        onClick={directory ? onSelect : onOpen}
        onDoubleClick={onOpen}
        onKeyDown={(event) => {
          if (event.key === "Enter" && onOpen) {
            event.preventDefault();
            onOpen();
          }
        }}
      >
        <ListItemAvatar>
          <DirectoryThumbnail />
        </ListItemAvatar>
        <ListItemText primary={name} />
      </ListItemButton>
    </ListItem>
  );
}

function ResourceRow({
  resource,
  disabled,
  selected,
  onSelect,
  onDownload,
  onDelete,
}: {
  resource: Resource;
  disabled: boolean;
  selected: boolean;
  onSelect: () => void;
  onDownload: () => void;
  onDelete: () => void;
}) {
  const status = resourceStatusLabel(resource.state.effective);
  const items = [
    ...(resource.content ? [{ id: "download", label: "Download", onSelect: onDownload }] : []),
    { id: "delete", label: "Delete", destructive: true, onSelect: onDelete },
  ];
  return (
    <ListItem disablePadding secondaryAction={<RowActions items={items} disabled={disabled} />}>
      <ListItemButton disabled={disabled} selected={selected} onClick={onSelect}>
        <ListItemAvatar>
          <ResourceThumbnail />
        </ListItemAvatar>
        <ListItemText
          primary={resource.name}
          secondary={`${status}${formatBytes(resource.content?.size)} · ${formatDate(resource.updatedAt)}`}
        />
      </ListItemButton>
    </ListItem>
  );
}

function resourceStatusLabel(status: Resource["state"]["effective"]): string {
  switch (status) {
    case "deleted":
      return "Deleted · ";
    case "verifying":
      return "Verifying · ";
    case "verification_failed":
      return "Verification failed · ";
    case "no_content":
    case "ready":
      return "";
  }
}

function RowActions({
  items,
  disabled,
}: {
  items: { id: string; label: string; destructive?: boolean; onSelect: () => void }[];
  disabled: boolean;
}) {
  const [anchor, setAnchor] = React.useState<HTMLElement | null>(null);
  return (
    <>
      <IconButton
        size="small"
        disabled={disabled}
        aria-label="Open actions"
        onClick={(event) => setAnchor(event.currentTarget)}
      >
        <MoreVertIcon fontSize="small" />
      </IconButton>
      <Menu anchorEl={anchor} open={Boolean(anchor) && !disabled} onClose={() => setAnchor(null)}>
        {items.map((item) => (
          <MenuItem
            key={item.id}
            onClick={() => {
              setAnchor(null);
              item.onSelect();
            }}
            sx={item.destructive ? { color: "error.main" } : undefined}
          >
            {item.destructive ? <DeleteIcon fontSize="small" sx={{ mr: 1 }} /> : null}
            {item.label}
          </MenuItem>
        ))}
      </Menu>
    </>
  );
}
