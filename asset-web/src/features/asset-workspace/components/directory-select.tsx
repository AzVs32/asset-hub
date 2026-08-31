import ArrowDropDownIcon from "@mui/icons-material/ArrowDropDown";
import ChevronRightIcon from "@mui/icons-material/ChevronRight";
import FolderOutlinedIcon from "@mui/icons-material/FolderOutlined";
import {
  Alert,
  Box,
  Breadcrumbs,
  CircularProgress,
  Divider,
  Link,
  List,
  ListItemButton,
  ListItemIcon,
  ListItemText,
  Popover,
  TextField,
  Typography,
  type TextFieldProps,
} from "@mui/material";
import { useQuery } from "@tanstack/react-query";
import React from "react";
import { breadcrumbs, normalizeDirectory } from "@/domain/directory-path";
import type { ResourceFilters } from "@/domain/resource";
import { useAssetWorkspaceGateway } from "@/shared/api/gateway-context";
import { queryKeys } from "@/shared/api/query-keys";

type DirectorySelectProps = Omit<
  TextFieldProps,
  "children" | "onChange" | "select" | "value"
> & {
  value: string;
  onChange: (value: string) => void;
};

export function DirectorySelect({ value, onChange, ...props }: DirectorySelectProps) {
  const gateway = useAssetWorkspaceGateway();
  const [anchor, setAnchor] = React.useState<HTMLElement | null>(null);
  const open = Boolean(anchor);
  const directory = normalizeDirectory(value);
  const filters = React.useMemo<ResourceFilters>(
    () => ({
      directory,
      page: 1,
      limit: 1,
      query: "",
      kind: "",
      includeDeleted: false,
    }),
    [directory],
  );
  const listing = useQuery({
    queryKey: queryKeys.directory(filters),
    queryFn: ({ signal }) => gateway.listDirectory(filters, signal),
    enabled: open,
  });
  const pathBreadcrumbs = breadcrumbs(directory);

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
      <Popover
        open={open}
        anchorEl={anchor}
        onClose={() => setAnchor(null)}
        anchorOrigin={{ vertical: "bottom", horizontal: "left" }}
        transformOrigin={{ vertical: "top", horizontal: "left" }}
      >
        <Box sx={{ width: anchor?.clientWidth ?? 320, maxWidth: "calc(100vw - 32px)" }}>
          <Breadcrumbs
            separator={<ChevronRightIcon fontSize="small" />}
            aria-label="Selected directory"
            sx={{ px: 2, py: 1.5 }}
          >
            {pathBreadcrumbs.map((crumb, index) =>
              index === pathBreadcrumbs.length - 1 ? (
                <Typography key={crumb.path || "root"} variant="body2" color="text.primary">
                  {crumb.label}
                </Typography>
              ) : (
                <Link
                  key={crumb.path || "root"}
                  component="button"
                  type="button"
                  variant="body2"
                  underline="hover"
                  onClick={() => onChange(crumb.path)}
                >
                  {crumb.label}
                </Link>
              ),
            )}
          </Breadcrumbs>
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
    </>
  );
}

function formatDirectory(directory: string): string {
  return directory ? `/${directory}` : "/";
}
