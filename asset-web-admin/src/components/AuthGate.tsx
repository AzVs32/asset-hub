import React from "react";
import { Database, Loader2 } from "lucide-react";
import { request } from "../api";
import { TextInput } from "./forms";

export type CurrentUser = { id: string; username: string; is_admin: boolean };

export function AuthGate({
  children,
}: {
  children: (session: { user: CurrentUser; initialDirectory: string; logout: () => Promise<void> }) => React.ReactNode;
}) {
  const [user, setUser] = React.useState<CurrentUser | null>(null);
  const [initialDirectory, setInitialDirectory] = React.useState("");
  const [checking, setChecking] = React.useState(true);
  const [username, setUsername] = React.useState("");
  const [password, setPassword] = React.useState("");
  const [authError, setAuthError] = React.useState<string | null>(null);

  async function loadSession(currentUser: CurrentUser) {
    const grants = await request<Array<{ directory: string; permission: string }>>("/auth/directory-grants");
    setInitialDirectory(grants[0]?.directory ?? "");
    setUser(currentUser);
  }

  React.useEffect(() => {
    request<{ user: CurrentUser }>("/auth/me")
      .then((response) => loadSession(response.user))
      .catch(() => undefined)
      .finally(() => setChecking(false));
  }, []);

  async function login(event: React.FormEvent) {
    event.preventDefault();
    setAuthError(null);
    try {
      const response = await request<{ user: CurrentUser }>("/auth/login", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ username, password }),
      });
      setPassword("");
      await loadSession(response.user);
    } catch (error) {
      setAuthError(error instanceof Error ? error.message : "Login failed");
    }
  }

  async function logout() {
    await request<void>("/auth/logout", { method: "POST" });
    setUser(null);
    setInitialDirectory("");
  }

  if (checking) return <main className="auth-page"><Loader2 className="spin" aria-label="Checking session" /></main>;
  if (!user) {
    return (
      <main className="auth-page">
        <form className="auth-card" onSubmit={login}>
          <Database size={30} />
          <h1>Asset Hub</h1>
          <TextInput label="Username" value={username} onChange={setUsername} />
          <label className="field">
            <span>Password</span>
            <input type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="current-password" />
          </label>
          {authError && <p className="error-banner">{authError}</p>}
          <button className="primary-button" type="submit">Sign in</button>
        </form>
      </main>
    );
  }
  return <>{children({ user, initialDirectory, logout })}</>;
}
