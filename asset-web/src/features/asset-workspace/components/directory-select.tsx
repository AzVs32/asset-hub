import ArrowDropDownIcon from "@mui/icons-material/ArrowDropDown";
import ChevronRightIcon from "@mui/icons-material/ChevronRight";
import FolderOutlinedIcon from "@mui/icons-material/FolderOutlined";
import {
  Alert,
  Box,
  Button,
  CircularProgress,
  Divider,
  List,
  ListItemButton,
  ListItemIcon,
  ListItemText,
  Popover,
  Stack,
  TextField,
  type TextFieldProps,
  Typography,
} from "@mui/material";
import { useQuery } from "@tanstack/react-query";
import React from "react";
import type { DirectoryListingQuery } from "@/domain/directory";
import { normalizeDirectory, parentDirectory } from "@/domain/directory-path";
import { useAssetWorkspaceGateway } from "@/shared/api/gateway-context";
import { queryKeys } from "@/shared/api/query-keys";
import { formatDirectory } from "@/shared/format";

type DirectorySelectProps = Omit<TextFieldProps, "children" | "onChange" | "select" | "value"> & {
  value: string;
  onChange: (value: string) => void;
};

export function DirectorySelect({ value, onChange, ...props }: DirectorySelectProps) {
  const [anchor, setAnchor] = React.useState<HTMLElement | null>(null);
  const open = Boolean(anchor) && !props.disabled;
  const directory = normalizeDirectory(value);

  return (
    <>
      <TextField
        {...props}
        fullWidth
        label="Directory"
        value={formatDirectory(directory)}
        onClick={(event) => {
          if (!props.disabled) setAnchor(event.currentTarget);
        }}
        onKeyDown={(event) => {
          if (
            !props.disabled &&
            (event.key === "Enter" || event.key === " " || event.key === "ArrowDown")
          ) {
            event.preventDefault();
            setAnchor(event.currentTarget);
          }
        }}
        InputProps={{
          readOnly: true,
          endAdornment: <ArrowDropDownIcon color="action" />,
        }}
        inputProps={{
          "aria-haspopup": "dialog",
          "aria-expanded": open,
        }}
      />
      <DirectoryPickerPopover
        anchor={open ? anchor : null}
        directory={directory}
        onClose={() => setAnchor(null)}
        onChange={(next) => {
          if (!props.disabled) onChange(next);
        }}
      />
    </>
  );
}

function DirectoryPickerPopover({
  anchor,
  directory,
  onClose,
  onChange,
}: {
  anchor: HTMLElement | null;
  directory: string;
  onClose: () => void;
  onChange: (value: string) => void;
}) {
  const gateway = useAssetWorkspaceGateway();
  const open = Boolean(anchor);
  const filters = React.useMemo<DirectoryListingQuery>(
    () => ({ directory, page: 1, limit: 1 }),
    [directory],
  );
  const listing = useQuery({
    queryKey: queryKeys.directory(filters),
    queryFn: ({ signal }) => gateway.listDirectory(filters, signal),
    enabled: open,
  });
  const parent = parentDirectory(directory);

  return (
    <Popover
      open={open}
      anchorEl={anchor}
      onClose={onClose}
      anchorOrigin={{ vertical: "bottom", horizontal: "left" }}
      transformOrigin={{ vertical: "top", horizontal: "left" }}
    >
      <Box
        sx={{ width: Math.max(anchor?.clientWidth ?? 320, 320), maxWidth: "calc(100vw - 32px)" }}
      >
        <Stack direction="row" alignItems="center" spacing={1} sx={{ px: 2, py: 1 }}>
          <Typography variant="body2" color="text.primary" noWrap sx={{ flex: 1 }}>
            {formatDirectory(directory)}
          </Typography>
          {parent !== null ? (
            <Button size="small" onClick={() => onChange(parent)}>
              ..
            </Button>
          ) : null}
        </Stack>
        <Divider />
        {listing.isPending ? (
          <Box sx={{ display: "grid", placeItems: "center", minHeight: 96 }}>
            <CircularProgress size={24} />
          </Box>
        ) : listing.isError ? (
          <Alert severity="error" sx={{ m: 1.5 }}>
            Unable to load folders
          </Alert>
        ) : listing.data.folders.length > 0 ? (
          <List dense disablePadding sx={{ py: 0.5, maxHeight: 280, overflow: "auto" }}>
            {listing.data.folders.map((folder) => (
              <ListItemButton key={folder.id} onClick={() => onChange(folder.path)}>
                <ListItemIcon sx={{ minWidth: 36 }}>
                  <FolderOutlinedIcon fontSize="small" />
                </ListItemIcon>
                <ListItemText primary={folder.name} />
                <ChevronRightIcon fontSize="small" color="action" />
              </ListItemButton>
            ))}
          </List>
        ) : (
          <Typography variant="body2" color="text.secondary" sx={{ px: 2, py: 3 }}>
            No subfolders
          </Typography>
        )}
      </Box>
    </Popover>
  );
}
