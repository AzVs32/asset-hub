import { zodResolver } from "@hookform/resolvers/zod";
import StorageRoundedIcon from "@mui/icons-material/StorageRounded";
import {
  Alert,
  Avatar,
  Box,
  Button,
  CircularProgress,
  Paper,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import { Controller, useForm } from "react-hook-form";
import { z } from "zod";

const loginSchema = z.object({
  username: z.string().trim().min(1, "Username is required"),
  password: z.string().min(1, "Password is required"),
});

type LoginInput = z.infer<typeof loginSchema>;

export function LoginForm({
  onSubmit,
  error,
}: {
  onSubmit: (input: LoginInput) => Promise<void>;
  error: string | null;
}) {
  const form = useForm<LoginInput>({
    resolver: zodResolver(loginSchema),
    defaultValues: { username: "", password: "" },
  });

  return (
    <Box sx={{ minHeight: "100vh", display: "grid", placeItems: "center", p: 2 }}>
      <Paper sx={{ width: "100%", maxWidth: 400 }}>
        <Stack component="form" spacing={2.5} sx={{ p: 4 }} onSubmit={form.handleSubmit(onSubmit)}>
          <Stack direction="row" spacing={1.5} alignItems="center">
            <Avatar sx={{ bgcolor: "primary.main" }}>
              <StorageRoundedIcon />
            </Avatar>
            <Box>
              <Typography variant="h6">Asset Hub</Typography>
              <Typography variant="body2" color="text.secondary">
                Sign in to your workspace
              </Typography>
            </Box>
          </Stack>
          <Controller
            name="username"
            control={form.control}
            render={({ field, fieldState }) => {
              const { ref, ...rest } = field;
              return (
                <TextField
                  {...rest}
                  inputRef={ref}
                  label="Username"
                  autoComplete="username"
                  error={Boolean(fieldState.error)}
                  helperText={fieldState.error?.message}
                />
              );
            }}
          />
          <Controller
            name="password"
            control={form.control}
            render={({ field, fieldState }) => {
              const { ref, ...rest } = field;
              return (
                <TextField
                  {...rest}
                  inputRef={ref}
                  type="password"
                  label="Password"
                  autoComplete="current-password"
                  error={Boolean(fieldState.error)}
                  helperText={fieldState.error?.message}
                />
              );
            }}
          />
          {error ? <Alert severity="error">{error}</Alert> : null}
          <Button
            type="submit"
            variant="contained"
            disabled={form.formState.isSubmitting}
            startIcon={
              form.formState.isSubmitting ? (
                <CircularProgress size={16} color="inherit" />
              ) : undefined
            }
          >
            Sign in
          </Button>
        </Stack>
      </Paper>
    </Box>
  );
}
