import CreateNewFolderIcon from "@mui/icons-material/CreateNewFolder";
import FolderIcon from "@mui/icons-material/Folder";
import MoreVertIcon from "@mui/icons-material/MoreVert";
import RefreshIcon from "@mui/icons-material/Refresh";
import SearchIcon from "@mui/icons-material/Search";
import UploadFileIcon from "@mui/icons-material/UploadFile";
import {
  Alert,
  Avatar,
  Box,
  Button,
  Card,
  CardActions,
  CardHeader,
  Checkbox,
  CircularProgress,
  Divider,
  FormControlLabel,
  IconButton,
  InputAdornment,
  List,
  ListItem,
  ListItemAvatar,
  ListItemButton,
  ListItemText,
  Menu,
  MenuItem,
  Pagination,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import React from "react";
import { parentDirectory } from "@/domain/directory-path";
import type {
  Directory,
  DirectoryAction,
  DirectoryListing,
  Resource,
  ResourceAction,
  ResourceFilters,
  ResourceKind,
} from "@/domain/resource";
import { formatBytes, formatDate } from "@/domain/resource-draft";
import { usePluginKernel } from "@/kernel/plugin-kernel";
import { coreDirectoryWorkspaceSlots } from "@/kernel/slots";
import { DirectoryThumbnail, ResourceThumbnail } from "./asset-thumbnail";
import { KindSelect } from "./kind-select";

export function ResourceList({
  listing,
  kinds,
  filters,
  selectedId,
  selectedDirectoryId,
  loading,
  error,
  onFilters,
  onOpenDirectory,
  onSelect,
  onSelectDirectory,
  onAction,
  onRestore,
  onDirectoryAction,
  onRefresh,
  onUpload,
  onCreateFolder,
}: {
  listing: DirectoryListing | undefined;
  kinds: ResourceKind[];
  filters: ResourceFilters;
  selectedId: string | null;
  selectedDirectoryId: string | null;
  loading: boolean;
  error: unknown;
  onFilters: (patch: Partial<ResourceFilters>) => void;
  onOpenDirectory: (path: string) => void;
  onSelect: (resource: Resource) => void;
  onSelectDirectory: (directory: Directory) => void;
  onAction: (resource: Resource, action: ResourceAction) => void;
  onRestore: (resource: Resource) => void;
  onDirectoryAction: (directory: Directory, action: DirectoryAction) => void;
  onRefresh: () => void;
  onUpload: () => void;
  onCreateFolder: () => void;
}) {
  const total = listing?.resources.total ?? 0;
  const totalPages = Math.max(1, Math.ceil(total / filters.limit));
  const parent = parentDirectory(filters.directory);

  return (
    <Card sx={{ display: "flex", flexDirection: "column", minHeight: 0, minWidth: 0 }}>
      <CardHeader
        avatar={
          <Avatar sx={{ bgcolor: "primary.main" }}>
            <FolderIcon />
          </Avatar>
        }
        title={listing?.directory.name || "Root"}
        subheader={`${listing?.folders.length ?? 0} folders · ${total} assets`}
        action={
          <Stack direction="row" spacing={1}>
            <IconButton aria-label="Refresh" onClick={onRefresh}>
              <RefreshIcon />
            </IconButton>
            <Button startIcon={<CreateNewFolderIcon />} onClick={onCreateFolder}>
              New folder
            </Button>
            <Button variant="contained" startIcon={<UploadFileIcon />} onClick={onUpload}>
              Upload
            </Button>
          </Stack>
        }
      />
      <Divider />
      <Box
        sx={{
          display: "grid",
          gridTemplateColumns: { xs: "1fr", md: "1fr 1fr auto" },
          gap: 1.5,
          p: 2,
        }}
      >
        <TextField
          size="small"
          placeholder="Search resources"
          value={filters.query}
          onChange={(event) => onFilters({ query: event.target.value, page: 1 })}
          InputProps={{
            startAdornment: (
              <InputAdornment position="start">
                <SearchIcon fontSize="small" />
              </InputAdornment>
            ),
          }}
          inputProps={{ "aria-label": "Search resources" }}
        />
        <KindSelect
          label="Resource kind"
          kinds={kinds}
          emptyOption={{ label: "All kinds" }}
          size="small"
          value={filters.kind}
          onChange={(event) => onFilters({ kind: event.target.value, page: 1 })}
        />
        <FormControlLabel
          control={
            <Checkbox
              checked={filters.includeDeleted}
              onChange={(event) => onFilters({ includeDeleted: event.target.checked, page: 1 })}
            />
          }
          label="Deleted"
        />
      </Box>
      <Divider />
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
          {parent !== null ? <FolderRow name=".." onClick={() => onOpenDirectory(parent)} /> : null}
          {listing?.folders.map((folder) => (
            <FolderRow
              key={folder.id}
              name={folder.name}
              directory={folder}
              selected={folder.id === selectedDirectoryId}
              onSelect={() => onSelectDirectory(folder)}
              onOpen={() => onOpenDirectory(folder.path)}
              onAction={(action) => onDirectoryAction(folder, action)}
            />
          ))}
          {listing?.resources.items.map((resource) => (
            <ResourceRow
              key={resource.id}
              resource={resource}
              selected={resource.id === selectedId}
              onSelect={() => onSelect(resource)}
              onAction={(action) => onAction(resource, action)}
              onRestore={() => onRestore(resource)}
            />
          ))}
        </List>
        {!loading && listing && !listing.folders.length && !listing.resources.items.length ? (
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
          page={filters.page}
          size="small"
          onChange={(_event, page) => onFilters({ page })}
        />
      </CardActions>
    </Card>
  );
}

function FolderRow({
  name,
  directory,
  selected = false,
  onClick,
  onSelect,
  onOpen,
  onAction,
}: {
  name: string;
  directory?: Directory;
  selected?: boolean;
  onClick?: () => void;
  onSelect?: () => void;
  onOpen?: () => void;
  onAction?: (action: DirectoryAction) => void;
}) {
  const kernel = usePluginKernel();
  const actions = directory
    ? kernel.directoryActionsAtCoreSlot(directory, coreDirectoryWorkspaceSlots.directoryContextMenu)
    : [];
  return (
    <ListItem
      disablePadding
      secondaryAction={
        actions.length && onAction ? (
          <ActionsMenu
            items={actions.map((action) => ({
              id: action.id,
              label: action.label,
              destructive: action.ui.destructive,
              onSelect: () => onAction(action),
            }))}
          />
        ) : null
      }
    >
      <ListItemButton
        selected={selected}
        aria-pressed={directory ? selected : undefined}
        onClick={onSelect ?? onClick}
        onDoubleClick={onOpen}
        onKeyDown={(event) => {
          if (event.key === "Enter" && onOpen) {
            event.preventDefault();
            onOpen();
          }
        }}
      >
        <ListItemAvatar>
          {directory ? (
            <DirectoryThumbnail directory={directory} />
          ) : (
            <Avatar
              sx={{
                color: "warning.dark",
                background: "linear-gradient(145deg, #fffbeb, #ffedd5)",
              }}
            >
              <FolderIcon />
            </Avatar>
          )}
        </ListItemAvatar>
        <ListItemText primary={name} secondary={directory?.kind ?? "Parent directory"} />
      </ListItemButton>
    </ListItem>
  );
}

function ResourceRow({
  resource,
  selected,
  onSelect,
  onAction,
  onRestore,
}: {
  resource: Resource;
  selected: boolean;
  onSelect: () => void;
  onAction: (action: ResourceAction) => void;
  onRestore: () => void;
}) {
  const kernel = usePluginKernel();
  const actions = kernel.resourceActionsAtCoreSlot(
    resource,
    coreDirectoryWorkspaceSlots.resourceContextMenu,
  );
  const status =
    resource.content?.verificationStatus === "pending"
      ? "Verifying · "
      : resource.content?.verificationStatus === "failed"
        ? "Verification failed · "
        : resource.deletedAt
          ? "Deleted · "
          : "";
  const menuItems = [
    ...(resource.deletedAt
      ? [{ id: "restore", label: "Restore resource", destructive: false, onSelect: onRestore }]
      : []),
    ...actions.map((action) => ({
      id: action.id,
      label: action.label,
      destructive: action.ui.destructive,
      onSelect: () => onAction(action),
    })),
  ];
  return (
    <ListItem
      disablePadding
      secondaryAction={menuItems.length ? <ActionsMenu items={menuItems} /> : null}
    >
      <ListItemButton selected={selected} onClick={onSelect}>
        <ListItemAvatar>
          <ResourceThumbnail resource={resource} />
        </ListItemAvatar>
        <ListItemText
          primary={resource.name}
          secondary={`${status}${resource.kind} · ${formatBytes(resource.content?.size ?? 0)} · ${formatDate(resource.updatedAt)}`}
        />
      </ListItemButton>
    </ListItem>
  );
}

function ActionsMenu({
  items,
}: {
  items: { id: string; label: string; destructive?: boolean; onSelect: () => void }[];
}) {
  const [anchor, setAnchor] = React.useState<HTMLElement | null>(null);
  return (
    <>
      <IconButton
        size="small"
        aria-label="Open actions"
        onClick={(event) => setAnchor(event.currentTarget)}
      >
        <MoreVertIcon fontSize="small" />
      </IconButton>
      <Menu anchorEl={anchor} open={Boolean(anchor)} onClose={() => setAnchor(null)}>
        {items.map((item) => (
          <MenuItem
            key={item.id}
            onClick={() => {
              setAnchor(null);
              item.onSelect();
            }}
            sx={item.destructive ? { color: "error.main" } : undefined}
          >
            {item.label}
          </MenuItem>
        ))}
      </Menu>
    </>
  );
}
