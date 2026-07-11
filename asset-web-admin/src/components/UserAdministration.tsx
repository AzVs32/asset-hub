import React from "react";
import { Loader2, X } from "lucide-react";
import { request } from "../api";
import { TextInput } from "./forms";

type ManagedUser = {
  id: string;
  username: string;
  role: "administrator" | "member";
  status: "active" | "disabled";
};
type DirectoryGrant = { directory: string; permission: "read" | "write" | "manage" };

export function UserAdministration({ currentUserId, onClose }: { currentUserId: string; onClose: () => void }) {
  const [users, setUsers] = React.useState<ManagedUser[]>([]);
  const [username, setUsername] = React.useState("");
  const [password, setPassword] = React.useState("");
  const [directory, setDirectory] = React.useState("");
  const [permission, setPermission] = React.useState("read");
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
      const response = await request<{ user: { id: string } }>("/auth/users", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ username, password, is_admin: isAdmin }),
      });
      if (!isAdmin && directory.trim()) {
        await request<void>("/auth/directory-grants", {
          method: "PUT",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ user_id: response.user.id, directory: directory.trim(), permission }),
        });
      }
      setUsername(""); setPassword(""); setDirectory(""); setIsAdmin(false);
      await loadUsers();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to create user");
    } finally { setBusy(false); }
  }

  async function updateUser(user: ManagedUser, patch: Partial<ManagedUser>) {
    setBusy(true); setError(null);
    try {
      await request<ManagedUser>(`/auth/users/${user.id}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ role: patch.role ?? user.role, status: patch.status ?? user.status }),
      });
      await loadUsers();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to update user");
    } finally { setBusy(false); }
  }

  return (
    <div className="modal-backdrop">
      <section className="modal user-admin-modal" aria-label="User administration">
        <header className="modal-header"><h2>Users and access</h2><button className="icon-button" onClick={onClose} title="Close"><X /></button></header>
        {error && <p className="error-banner">{error}</p>}
        <div className="user-list">
          {users.map((user) => (
            <div className="user-row" key={user.id}>
              <strong>{user.username}</strong>
              <select value={user.role} disabled={busy || user.id === currentUserId} onChange={(event) => updateUser(user, { role: event.target.value as ManagedUser["role"] })}>
                <option value="member">Member</option><option value="administrator">Administrator</option>
              </select>
              <select value={user.status} disabled={busy || user.id === currentUserId} onChange={(event) => updateUser(user, { status: event.target.value as ManagedUser["status"] })}>
                <option value="active">Active</option><option value="disabled">Disabled</option>
              </select>
              {user.role === "member" && <button className="ghost-button" type="button" onClick={() => void selectGrantUser(user)}>Access</button>}
            </div>
          ))}
        </div>
        {grantUser && <form className="admin-create-form" onSubmit={saveGrant}>
          <h3>Directory access for {grantUser.username}</h3>
          {grants.map((grant) => <div className="grant-row" key={grant.directory}><code>{grant.directory || "/"}</code><span>{grant.permission}</span><button type="button" className="ghost-button" disabled={busy} onClick={() => void revokeGrant(grant)}>Revoke</button></div>)}
          <TextInput label="Directory" value={grantDirectory} onChange={setGrantDirectory} />
          <label className="field"><span>Permission</span><select value={grantPermission} onChange={(event) => setGrantPermission(event.target.value as DirectoryGrant["permission"])}><option value="read">Read</option><option value="write">Write</option><option value="manage">Manage</option></select></label>
          <button className="primary-button" disabled={busy}>Save access</button>
        </form>}
        <form className="admin-create-form" onSubmit={createUser}>
          <h3>Create user</h3>
          <TextInput label="Username" value={username} onChange={setUsername} />
          <label className="field"><span>Password</span><input type="password" minLength={10} value={password} onChange={(event) => setPassword(event.target.value)} /></label>
          <label className="toggle-field"><input type="checkbox" checked={isAdmin} onChange={(event) => setIsAdmin(event.target.checked)} />Administrator</label>
          {!isAdmin && <><TextInput label="Initial directory" value={directory} onChange={setDirectory} /><label className="field"><span>Permission</span><select value={permission} onChange={(event) => setPermission(event.target.value)}><option value="read">Read</option><option value="write">Write</option><option value="manage">Manage</option></select></label></>}
          <button className="primary-button" disabled={busy}>{busy && <Loader2 className="spin" size={16} />}Create</button>
        </form>
      </section>
    </div>
  );
}
