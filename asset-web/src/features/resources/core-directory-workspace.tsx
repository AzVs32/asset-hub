import { Box } from "@mui/material";
import type React from "react";

/**
 * The built-in Directory UI subtree. Its row menus, thumbnails, filters, and detail panel are
 * Core-owned implementation slots and are absent when a plugin owns directory_workspace.
 */
export function CoreDirectoryWorkspace({
  browser,
  detail,
}: {
  browser: React.ReactNode;
  detail: React.ReactNode;
}) {
  return (
    <Box
      sx={{
        display: "grid",
        gridTemplateColumns: { xs: "1fr", lg: "minmax(0, 1fr) 24rem" },
        gridTemplateRows: { xs: "auto auto", lg: "minmax(0, 1fr)" },
        gap: 2,
        flex: 1,
        minHeight: 0,
        overflow: { xs: "visible", lg: "hidden" },
        p: 2,
      }}
    >
      {browser}
      {detail}
    </Box>
  );
}
