import {
  Alert,
  Box,
  Button,
  Checkbox,
  Chip,
  CircularProgress,
  Dialog,
  DialogContent,
  DialogTitle,
  FormControlLabel,
  MenuItem,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Controller, useForm } from "react-hook-form";
import { toast } from "sonner";
import { useGateway } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
import type { ManagedUser } from "@/domain/auth";

interface NewUserForm {
  username: string;
  password: string;
  isAdmin: boolean;
}

export function UserAdministration({
  open,
  onOpenChange,
  currentUserId,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  currentUserId: string;
}) {
  const gateway = useGateway();
  const queryClient = useQueryClient();
  const users = useQuery({
    queryKey: queryKeys.users,
    queryFn: () => gateway.listUsers(),
    enabled: open,
  });
  const newUser = useForm<NewUserForm>({
    defaultValues: { username: "", password: "", isAdmin: false },
  });

  const createUser = useMutation({
    mutationFn: gateway.createUser.bind(gateway),
    onSuccess: async () => {
      toast.success("User created");
      newUser.reset();
      await queryClient.invalidateQueries({ queryKey: queryKeys.users });
    },
    onError: notifyError,
  });
  const updateStatus = useMutation({
    mutationFn: ({ user, status }: { user: ManagedUser; status: ManagedUser["status"] }) =>
      gateway.updateUserStatus(user.id, status),
    onSuccess: async () => {
      toast.success("User status updated");
      await queryClient.invalidateQueries({ queryKey: queryKeys.users });
    },
    onError: notifyError,
  });
  const busy = createUser.isPending || updateStatus.isPending;

  return (
    <Dialog open={open} fullWidth maxWidth="md" onClose={() => onOpenChange(false)}>
      <DialogTitle>Users</DialogTitle>
      <DialogContent>
        <Stack spacing={3.5} sx={{ pt: 1 }}>
          {users.isPending ? (
            <Box sx={{ display: "flex", justifyContent: "center", p: 2 }}>
              <CircularProgress size={24} />
            </Box>
          ) : null}
          {users.isError ? (
            <Alert severity="error">
              {users.error instanceof Error ? users.error.message : "Unexpected error"}
            </Alert>
          ) : null}
          <Stack spacing={1}>
            {users.data?.map((user) => (
              <Box
                key={user.id}
                sx={{
                  display: "grid",
                  gridTemplateColumns: { xs: "1fr", sm: "minmax(150px, 1fr) 7rem 9rem" },
                  gap: 1.5,
                  alignItems: "center",
                  borderRadius: 2,
                  border: 1,
                  borderColor: "divider",
                  p: 2,
                }}
              >
                <Box sx={{ minWidth: 0 }}>
                  <Typography component="span" variant="body2" fontWeight={600}>
                    {user.username}
                  </Typography>
                  {user.id === currentUserId ? (
                    <Chip
                      label="You"
                      size="small"
                      color="primary"
                      variant="outlined"
                      sx={{ ml: 1 }}
                    />
                  ) : null}
                  <Typography
                    component="code"
                    variant="caption"
                    sx={{ display: "block", color: "text.secondary", mt: 0.5 }}
                  >
                    /{user.workspaceDirectory}
                  </Typography>
                </Box>
                <Typography variant="body2" sx={{ textTransform: "capitalize" }}>
                  {user.role}
                </Typography>
                <TextField
                  select
                  size="small"
                  value={user.status}
                  disabled={busy || user.id === currentUserId}
                  onChange={(event) =>
                    updateStatus.mutate({
                      user,
                      status: event.target.value as ManagedUser["status"],
                    })
                  }
                >
                  <MenuItem value="active">Active</MenuItem>
                  <MenuItem value="disabled">Disabled</MenuItem>
                </TextField>
              </Box>
            ))}
          </Stack>

          <Stack
            component="form"
            spacing={2}
            sx={{ borderRadius: 2, border: 1, borderColor: "divider", p: 2.5 }}
            onSubmit={newUser.handleSubmit((input) => createUser.mutate(input))}
          >
            <Box>
              <Typography variant="subtitle1" fontWeight={700}>
                Create user
              </Typography>
              <Typography variant="caption" color="text.secondary">
                Add an account with its own workspace.
              </Typography>
            </Box>
            <Controller
              name="username"
              control={newUser.control}
              render={({ field }) => {
                const { ref, ...rest } = field;
                return <TextField {...rest} inputRef={ref} label="Username" />;
              }}
            />
            <Controller
              name="password"
              control={newUser.control}
              render={({ field }) => {
                const { ref, ...rest } = field;
                return <TextField {...rest} inputRef={ref} type="password" label="Password" />;
              }}
            />
            <Controller
              name="isAdmin"
              control={newUser.control}
              render={({ field }) => (
                <FormControlLabel
                  control={<Checkbox checked={field.value} onChange={field.onChange} />}
                  label="Administrator"
                />
              )}
            />
            <Box>
              <Button type="submit" variant="contained" disabled={busy}>
                {createUser.isPending ? "Creating…" : "Create user"}
              </Button>
            </Box>
          </Stack>
        </Stack>
      </DialogContent>
    </Dialog>
  );
}

function notifyError(error: unknown) {
  toast.error(error instanceof Error ? error.message : "Request failed");
}
