import { Alert, Box, Button, CircularProgress, Stack, Typography } from "@mui/material";
import { isRouteErrorResponse, useRouteError } from "react-router";

export function RouteLoading() {
  return (
    <Box
      role="status"
      aria-label="Loading workspace"
      sx={{ display: "grid", placeItems: "center", minHeight: "100dvh" }}
    >
      <CircularProgress />
    </Box>
  );
}

export function RouteError() {
  const error = useRouteError();
  const message = isRouteErrorResponse(error)
    ? `${error.status}: ${error.statusText}`
    : "The workspace could not be loaded. Reload to try again.";
  return (
    <Box component="main" sx={{ maxWidth: 640, mx: "auto", p: 3 }}>
      <Stack spacing={2}>
        <Typography variant="h5" component="h1">
          Workspace unavailable
        </Typography>
        <Alert severity="error">{message}</Alert>
        <Button variant="contained" onClick={() => window.location.reload()}>
          Reload workspace
        </Button>
        <Button href="/">Open root directory</Button>
      </Stack>
    </Box>
  );
}
