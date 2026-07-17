import React from "react";
import { Loader2, X } from "lucide-react";
import { request } from "../api";
import type { DirectoryAccessEntry, ManagedUser } from "../api/contracts";
import { TextInput } from "./forms";
import { iconButtonClass, inputClass, primaryButtonClass, secondaryButtonClass } from "./ui";

type DirectoryGrant = DirectoryAccessEntry;

export function UserAdministration({ currentUserId, onClose }: { currentUserId: string; onClose: () => void }) {
  const [users, setUsers] = React.useState<ManagedUser[]>([]);
  const [username, setUsername] = React.useState("");
  const [password, setPassword] = React.useState("");
  const [directory, setDirectory] = React.useState("");
  const [isAdmin, setIsAdmin] = React.useState(false);
  const [grantUser, setGrantUser] = React.useState<ManagedUser | null>(null);
  const [grants, setGrants] = React.useState<DirectoryGrant[]>([]);
  const [grantDirectory, setGrantDirectory] = React.useState("");
  const [grantPermission, setGrantPermission] = React.useState<DirectoryGrant["permission"]>("read");
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const loadUsers = React.useCallback(async () => {
    setUsers(await request<ManagedUser[]>("/auth/users"));
  }, []);

  React.useEffect(() => { void loadUsers(); }, [loadUsers]);

  async function selectGrantUser(user: ManagedUser) {
    setGrantUser(user);
    setGrants(await request<DirectoryGrant[]>(`/auth/directory-grants?user_id=${encodeURIComponent(user.id)}`));
  }

  async function saveGrant(event: React.FormEvent) {
    event.preventDefault();
    if (!grantUser) return;
    setBusy(true); setError(null);
    try {
      await request<void>("/auth/directory-grants", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ user_id: grantUser.id, directory: grantDirectory.trim(), permission: grantPermission }),
      });
      setGrantDirectory("");
      await selectGrantUser(grantUser);
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Unable to save grant"); }
    finally { setBusy(false); }
  }

  async function revokeGrant(grant: DirectoryGrant) {
    if (!grantUser) return;
    setBusy(true); setError(null);
    try {
      const query = new URLSearchParams({ user_id: grantUser.id, directory: grant.directory });
      await request<void>(`/auth/directory-grants?${query}`, { method: "DELETE" });
      await selectGrantUser(grantUser);
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Unable to revoke grant"); }
    finally { setBusy(false); }
  }

  async function createUser(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await request<void>("/auth/users", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          username,
          password,
          is_admin: isAdmin,
          workspace_directory: isAdmin ? undefined : directory.trim(),
        }),
      });
      setUsername(""); setPassword(""); setDirectory(""); setIsAdmin(false);
      await loadUsers();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to create user");
    } finally { setBusy(false); }
  }

  async function updateUserStatus(user: ManagedUser, status: ManagedUser["status"]) {
    setBusy(true); setError(null);
    try {
      await request<ManagedUser>(`/auth/users/${user.id}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ status }),
      });
      await loadUsers();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to update user");
    } finally { setBusy(false); }
  }

  return (
    <div className="fixed inset-0 z-50 grid place-items-center overflow-y-auto bg-slate-950/50 p-4 backdrop-blur-sm">
      <section className="max-h-[calc(100vh-2rem)] w-full max-w-3xl overflow-y-auto rounded-2xl bg-white shadow-2xl" aria-label="User administration">
        <header className="sticky top-0 z-10 flex items-center justify-between border-b border-slate-200 bg-white/95 px-6 py-5 backdrop-blur"><div><h2 className="text-xl font-bold text-slate-900">Users and access</h2><p className="mt-1 text-sm text-slate-500">Manage accounts and directory permissions</p></div><button className={iconButtonClass} onClick={onClose} title="Close"><X size={18} /></button></header>
        <div className="grid gap-8 p-6">
        {error && <p className="rounded-lg border border-red-200 bg-red-50 p-3 text-sm text-red-700">{error}</p>}
        <div className="grid gap-2">
          {users.map((user) => (
            <div className="grid items-center gap-3 rounded-xl border border-slate-200 p-3 sm:grid-cols-[minmax(160px,1fr)_8rem_9rem_auto]" key={user.id}>
              <div className="min-w-0"><strong className="text-sm text-slate-900">{user.username}</strong>{user.id === currentUserId && <span className="ml-2 rounded-full bg-blue-50 px-2 py-0.5 text-xs font-medium text-blue-700">You</span>}<code className="mt-1 block truncate text-xs text-slate-500">{user.workspace_directory || "/"}</code></div>
              <span className="text-sm capitalize text-slate-600">{user.role}</span>
              <select className={inputClass} value={user.status} disabled={busy || user.id === currentUserId} onChange={(event) => updateUserStatus(user, event.target.value as ManagedUser["status"])}>
                <option value="active">Active</option><option value="disabled">Disabled</option>
              </select>
              {user.role === "member" && <button className={secondaryButtonClass} type="button" onClick={() => void selectGrantUser(user)}>Access</button>}
            </div>
          ))}
        </div>
        {grantUser && <form className="grid gap-4 rounded-xl border border-slate-200 bg-slate-50 p-5" onSubmit={saveGrant}>
          <h3 className="font-semibold text-slate-900">Directory access for {grantUser.username}</h3>
          {grants.map((grant) => <div className="grid grid-cols-[1fr_auto_auto] items-center gap-3 rounded-lg bg-white p-3 text-sm" key={grant.directory}><code className="truncate text-slate-700">{grant.directory || "/"}</code><span className="rounded-full bg-slate-100 px-2 py-1 text-xs font-medium text-slate-600">{grant.is_workspace ? "Workspace · " : ""}{grant.permission}</span><button type="button" className={secondaryButtonClass} disabled={busy || grant.is_workspace} onClick={() => void revokeGrant(grant)}>Revoke</button></div>)}
          <TextInput label="Directory" value={grantDirectory} onChange={setGrantDirectory} />
          <label className="grid gap-2"><span className="text-xs font-semibold text-slate-600">Permission</span><select className={inputClass} value={grantPermission} onChange={(event) => setGrantPermission(event.target.value as DirectoryGrant["permission"])}><option value="read">Read</option><option value="write">Write</option><option value="full">Full</option></select></label>
          <button className={primaryButtonClass} disabled={busy}>Save access</button>
        </form>}
        <form className="grid gap-4 border-t border-slate-200 pt-6" onSubmit={createUser}>
          <h3 className="font-semibold text-slate-900">Create user</h3>
          <TextInput label="Username" value={username} onChange={setUsername} />
          <label className="grid gap-2"><span className="text-xs font-semibold text-slate-600">Password</span><input className={inputClass} type="password" minLength={10} value={password} onChange={(event) => setPassword(event.target.value)} /></label>
          <label className="flex items-center gap-2 text-sm font-medium text-slate-700"><input className="size-4 rounded border-slate-300 text-blue-600" type="checkbox" checked={isAdmin} onChange={(event) => setIsAdmin(event.target.checked)} />Administrator</label>
          {!isAdmin && <TextInput label="Workspace directory" value={directory} onChange={setDirectory} />}
          <button className={primaryButtonClass} disabled={busy}>{busy && <Loader2 className="animate-spin" size={16} />}Create</button>
        </form>
        </div>
      </section>
    </div>
  );
}
