import CreateNewFolderIcon from "@mui/icons-material/CreateNewFolder";
import {
  Alert,
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  TextField,
  Typography,
} from "@mui/material";
import React from "react";
import { Controller, useForm } from "react-hook-form";

export function CreateFolderDialog({
  open,
  onOpenChange,
  parent,
  pending,
  onCreate,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  parent: string;
  pending: boolean;
  onCreate: (name: string) => Promise<unknown>;
}) {
  const form = useForm<{ name: string }>({
    defaultValues: { name: "" },
  });
  React.useEffect(() => {
    if (open) form.reset();
  }, [form, open]);
  return (
    <Dialog
      open={open}
      fullWidth
      maxWidth="xs"
      onClose={() => {
        if (!pending) onOpenChange(false);
      }}
    >
      <DialogTitle>New folder</DialogTitle>
      <DialogContent>
        <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 2 }}>
          Inside /{parent}
        </Typography>
        <Box
          component="form"
          sx={{ display: "grid", gap: 2, pt: 1 }}
          onSubmit={form.handleSubmit(async ({ name }) => {
            try {
              await onCreate(name);
              onOpenChange(false);
            } catch (error) {
              form.setError("root", {
                message: error instanceof Error ? error.message : "Folder creation failed",
              });
            }
          })}
        >
          <Controller
            name="name"
            control={form.control}
            rules={{
              validate: (value) => value.trim().length > 0 || "Folder name is required",
            }}
            render={({ field, fieldState }) => {
              const { ref, ...rest } = field;
              return (
                <TextField
                  {...rest}
                  inputRef={ref}
                  disabled={pending}
                  label="Folder name"
                  autoFocus
                  error={Boolean(fieldState.error)}
                  helperText={fieldState.error?.message}
                />
              );
            }}
          />
          {form.formState.errors.root ? (
            <Alert severity="error">{form.formState.errors.root.message}</Alert>
          ) : null}
          <DialogActions sx={{ px: 0, pb: 0 }}>
            <Button disabled={pending} onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button
              type="submit"
              variant="contained"
              startIcon={<CreateNewFolderIcon />}
              disabled={pending}
            >
              {pending ? "Creating…" : "Create"}
            </Button>
          </DialogActions>
        </Box>
      </DialogContent>
    </Dialog>
  );
}
