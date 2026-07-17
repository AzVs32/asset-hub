import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import React from "react";
import { useForm } from "react-hook-form";
import { toast } from "sonner";
import { useGateway } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
import type { DirectoryPermission, ManagedUser } from "@/domain/auth";
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

interface GrantForm {
  directory: string;
  permission: DirectoryPermission;
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
  const [selected, setSelected] = React.useState<ManagedUser | null>(null);
  const users = useQuery({
    queryKey: queryKeys.users,
    queryFn: () => gateway.listUsers(),
    enabled: open,
  });
  const grants = useQuery({
    queryKey: queryKeys.grants(selected?.id),
    queryFn: () => gateway.listDirectoryGrants(selected?.id),
    enabled: open && Boolean(selected),
  });
  const newUser = useForm<NewUserForm>({
    defaultValues: { username: "", password: "", workspaceDirectory: "", isAdmin: false },
  });
  const grantForm = useForm<GrantForm>({ defaultValues: { directory: "", permission: "read" } });

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
  const saveGrant = useMutation({
    mutationFn: ({ userId, input }: { userId: string; input: GrantForm }) =>
      gateway.grantDirectory(userId, input.directory.trim(), input.permission),
    onSuccess: async () => {
      toast.success("Directory access saved");
      grantForm.reset();
      await queryClient.invalidateQueries({ queryKey: queryKeys.grants(selected?.id) });
    },
    onError: notifyError,
  });
  const revokeGrant = useMutation({
    mutationFn: ({ userId, directory }: { userId: string; directory: string }) =>
      gateway.revokeDirectory(userId, directory),
    onSuccess: async () => {
      toast.success("Directory access revoked");
      await queryClient.invalidateQueries({ queryKey: queryKeys.grants(selected?.id) });
    },
    onError: notifyError,
  });
  const busy =
    createUser.isPending || updateStatus.isPending || saveGrant.isPending || revokeGrant.isPending;

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="Users and access"
      description="Accounts, workspaces, and explicit directory grants."
      className="max-w-4xl"
    >
      <div className="grid gap-7 p-6">
        {users.isPending ? <LoadingState label="Loading users" compact /> : null}
        {users.isError ? <ErrorState error={users.error} compact /> : null}
        <div className="grid gap-2">
          {users.data?.map((user) => (
            <div
              className="grid items-center gap-3 rounded-xl border border-slate-200 p-3 sm:grid-cols-[minmax(150px,1fr)_7rem_9rem_auto]"
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
              {user.role === "member" ? (
                <Button variant="secondary" size="small" onClick={() => setSelected(user)}>
                  Access
                </Button>
              ) : (
                <span />
              )}
            </div>
          ))}
        </div>

        {selected ? (
          <section className="grid gap-4 rounded-2xl border border-slate-200 bg-slate-50 p-5">
            <h3 className="font-semibold text-slate-900">Directory access · {selected.username}</h3>
            {grants.data?.map((grant) => (
              <div
                className="grid grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-3 rounded-xl bg-white p-3 text-sm"
                key={grant.directory}
              >
                <code className="truncate">/{grant.directory}</code>
                <span className="rounded-full bg-slate-100 px-2 py-1 text-xs">
                  {grant.isWorkspace ? "Workspace · " : ""}
                  {grant.permission}
                </span>
                <Button
                  variant="ghost"
                  size="small"
                  className="text-red-600"
                  disabled={busy || grant.isWorkspace}
                  onClick={() =>
                    revokeGrant.mutate({ userId: selected.id, directory: grant.directory })
                  }
                >
                  Revoke
                </Button>
              </div>
            ))}
            <form
              className="grid gap-3 sm:grid-cols-[1fr_10rem_auto]"
              onSubmit={grantForm.handleSubmit((input) =>
                saveGrant.mutate({ userId: selected.id, input }),
              )}
            >
              <Field label="Directory">
                <Input {...grantForm.register("directory", { required: true })} />
              </Field>
              <Field label="Permission">
                <select className={controlClass} {...grantForm.register("permission")}>
                  <option value="read">Read</option>
                  <option value="write">Write</option>
                  <option value="full">Full</option>
                </select>
              </Field>
              <Button className="self-end" type="submit" disabled={busy}>
                Save access
              </Button>
            </form>
          </section>
        ) : null}

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
