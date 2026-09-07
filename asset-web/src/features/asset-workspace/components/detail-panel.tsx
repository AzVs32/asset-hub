import ContentCopyIcon from "@mui/icons-material/ContentCopy";
import { Box, Divider, IconButton, Paper, Stack, Tooltip, Typography } from "@mui/material";
import type React from "react";
import { toast } from "sonner";

export function DetailPanel({
  thumbnail,
  title,
  subtitle,
  badges,
  action,
  children,
}: {
  thumbnail: React.ReactNode;
  title: string;
  subtitle: string;
  badges?: React.ReactNode;
  action?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <Paper
      component="aside"
      aria-label={`${title} details`}
      sx={{
        display: "flex",
        flexDirection: "column",
        minHeight: 0,
        overflow: "hidden",
        backgroundColor: "background.paper",
        backgroundImage: "none",
        boxShadow: "0 16px 40px -34px rgba(30, 41, 59, 0.65)",
      }}
    >
      <Box
        sx={{
          display: "grid",
          gridTemplateColumns: "auto minmax(0, 1fr) auto",
          alignItems: "start",
          gap: 1.5,
          p: 2,
        }}
      >
        {thumbnail}
        <Box sx={{ minWidth: 0 }}>
          <Typography variant="subtitle1" sx={{ lineHeight: 1.25, wordBreak: "break-word" }}>
            {title}
          </Typography>
          <Typography
            variant="body2"
            color="text.secondary"
            title={subtitle}
            sx={{ mt: 0.25, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
          >
            {subtitle}
          </Typography>
          {badges ? (
            <Stack direction="row" spacing={0.75} useFlexGap flexWrap="wrap" sx={{ mt: 1 }}>
              {badges}
            </Stack>
          ) : null}
        </Box>
        {action ? <Box>{action}</Box> : null}
      </Box>
      <Divider />
      <Box sx={{ flex: 1, minHeight: 0, overflow: "auto" }}>{children}</Box>
    </Paper>
  );
}

export function DetailSection({
  title,
  children,
  divider = true,
}: {
  title: string;
  children: React.ReactNode;
  divider?: boolean;
}) {
  return (
    <>
      <Box component="section" sx={{ px: 2, py: 2 }}>
        <Typography
          component="h3"
          variant="caption"
          color="text.secondary"
          sx={{ display: "block", mb: 1.5, fontWeight: 800, letterSpacing: "0.08em" }}
        >
          {title.toUpperCase()}
        </Typography>
        <Stack spacing={1.25}>{children}</Stack>
      </Box>
      {divider ? <Divider /> : null}
    </>
  );
}

export function DetailRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <Box
      sx={{
        display: "grid",
        gridTemplateColumns: "7rem minmax(0, 1fr)",
        alignItems: "start",
        gap: 1.25,
      }}
    >
      <Typography variant="body2" color="text.secondary">
        {label}
      </Typography>
      <Box sx={{ minWidth: 0, wordBreak: "break-word" }}>
        {typeof children === "string" || typeof children === "number" ? (
          <Typography variant="body2">{children}</Typography>
        ) : (
          children
        )}
      </Box>
    </Box>
  );
}

export function DetailAdvanced({ children }: { children: React.ReactNode }) {
  return (
    <DetailSection title="Advanced" divider={false}>
      {children}
    </DetailSection>
  );
}

export function CopyableValue({ value }: { value: string }) {
  return (
    <Stack direction="row" spacing={0.5} alignItems="flex-start">
      <Typography
        variant="body2"
        title={value}
        sx={{ flex: 1, minWidth: 0, fontFamily: "monospace", wordBreak: "break-all" }}
      >
        {value}
      </Typography>
      <Tooltip title="Copy">
        <IconButton
          size="small"
          aria-label="Copy value"
          onClick={() => void copyValue(value)}
          sx={{ mt: -0.5, mr: -0.5 }}
        >
          <ContentCopyIcon fontSize="inherit" />
        </IconButton>
      </Tooltip>
    </Stack>
  );
}

async function copyValue(value: string): Promise<void> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(value);
      toast.success("Copied to clipboard");
      return;
    }
  } catch {
    // Fall back to selection-based copying when Clipboard API permission is unavailable.
  }
  if (copyWithSelection(value)) {
    toast.success("Copied to clipboard");
  } else {
    toast.error("Unable to copy to clipboard");
  }
}

function copyWithSelection(value: string): boolean {
  const input = document.createElement("textarea");
  input.value = value;
  input.setAttribute("readonly", "");
  input.style.position = "fixed";
  input.style.opacity = "0";
  document.body.append(input);
  input.select();
  try {
    return document.execCommand("copy");
  } catch {
    return false;
  } finally {
    input.remove();
  }
}

export function DetailEmptyState({ icon, message }: { icon: React.ReactNode; message: string }) {
  return (
    <Paper
      sx={{
        display: "grid",
        placeItems: "center",
        minHeight: 288,
        backgroundColor: "rgba(255, 255, 255, 0.72)",
        backgroundImage: "none",
        boxShadow: "none",
      }}
    >
      <Stack alignItems="center" spacing={1.5} sx={{ px: 3, textAlign: "center" }}>
        {icon}
        <Typography variant="body2" color="text.secondary">
          {message}
        </Typography>
      </Stack>
    </Paper>
  );
}
