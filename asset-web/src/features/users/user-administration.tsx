import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { toast } from "sonner";
import { useGateway } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
import type { ManagedUser } from "@/domain/auth";
import { Button } from "@/shared/ui/button";
import { Dialog } from "@/shared/ui/dialog";
import { controlClass, Field, Input } from "@/shared/ui/field";
import { ErrorState, LoadingState } from "@/shared/ui/state";

interface NewUserForm {
  username: string;
  password: string;
  workspaceDirectory: string;
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
    defaultValues: { username: "", password: "", workspaceDirectory: "", isAdmin: false },
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
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="Users"
      description="Accounts and their workspace boundaries."
      className="max-w-4xl"
    >
      <div className="grid gap-7 p-6">
        {users.isPending ? <LoadingState label="Loading users" compact /> : null}
        {users.isError ? <ErrorState error={users.error} compact /> : null}
        <div className="grid gap-2">
          {users.data?.map((user) => (
            <div
              className="grid items-center gap-3 rounded-xl border border-slate-200 p-3 sm:grid-cols-[minmax(150px,1fr)_7rem_9rem]"
              key={user.id}
            >
              <div className="min-w-0">
                <strong className="text-sm text-slate-900">{user.username}</strong>
                {user.id === currentUserId ? (
                  <span className="ml-2 rounded-full bg-blue-50 px-2 py-0.5 text-[11px] text-blue-700">
                    You
                  </span>
                ) : null}
                <code className="mt-1 block truncate text-xs text-slate-500">
                  /{user.workspaceDirectory}
                </code>
              </div>
              <span className="text-sm capitalize text-slate-600">{user.role}</span>
              <select
                className={controlClass}
                value={user.status}
                disabled={busy || user.id === currentUserId}
                onChange={(event) =>
                  updateStatus.mutate({ user, status: event.target.value as ManagedUser["status"] })
                }
              >
                <option value="active">Active</option>
                <option value="disabled">Disabled</option>
              </select>
            </div>
          ))}
        </div>

        <form
          className="grid gap-4 border-t border-slate-200 pt-6 sm:grid-cols-2"
          onSubmit={newUser.handleSubmit((input) => createUser.mutate(input))}
        >
          <h3 className="font-semibold text-slate-900 sm:col-span-2">Create user</h3>
          <Field label="Username">
            <Input {...newUser.register("username", { required: true })} />
          </Field>
          <Field label="Password">
            <Input
              type="password"
              minLength={10}
              {...newUser.register("password", { required: true })}
            />
          </Field>
          {!newUser.watch("isAdmin") ? (
            <Field label="Workspace directory">
              <Input {...newUser.register("workspaceDirectory", { required: true })} />
            </Field>
          ) : (
            <div />
          )}
          <label className="flex items-center gap-2 self-end pb-3 text-sm font-medium text-slate-700">
            <input
              className="size-4 accent-blue-600"
              type="checkbox"
              {...newUser.register("isAdmin")}
            />
            Administrator
          </label>
          <div className="sm:col-span-2">
            <Button type="submit" disabled={busy}>
              {createUser.isPending ? "Creating…" : "Create user"}
            </Button>
          </div>
        </form>
      </div>
    </Dialog>
  );
}

function notifyError(error: unknown) {
  toast.error(error instanceof Error ? error.message : "Request failed");
}
