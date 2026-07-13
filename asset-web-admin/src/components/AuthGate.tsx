import React from "react";
import { Database, Loader2 } from "lucide-react";
import { request } from "../api";
import { TextInput } from "./forms";
import { inputClass, primaryButtonClass } from "./ui";

export type CurrentUser = { id: string; username: string; is_admin: boolean; home_directory: string };

export function AuthGate({
  children,
}: {
  children: (session: { user: CurrentUser; initialDirectory: string; logout: () => Promise<void> }) => React.ReactNode;
}) {
  const [user, setUser] = React.useState<CurrentUser | null>(null);
  const [checking, setChecking] = React.useState(true);
  const [username, setUsername] = React.useState("");
  const [password, setPassword] = React.useState("");
  const [authError, setAuthError] = React.useState<string | null>(null);

  function loadSession(currentUser: CurrentUser) {
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
      loadSession(response.user);
    } catch (error) {
      setAuthError(error instanceof Error ? error.message : "Login failed");
    }
  }

  async function logout() {
    await request<void>("/auth/logout", { method: "POST" });
    setUser(null);
  }

  if (checking) return <main className="grid min-h-screen place-items-center bg-slate-100"><Loader2 className="animate-spin text-blue-600" aria-label="Checking session" /></main>;
  if (!user) {
    return (
      <main className="grid min-h-screen place-items-center bg-slate-100 p-6">
        <form className="grid w-full max-w-sm gap-5 rounded-2xl border border-slate-200 bg-white p-8 shadow-xl shadow-slate-900/5" onSubmit={login}>
          <div className="flex size-12 items-center justify-center rounded-xl bg-blue-600 text-white"><Database size={24} /></div>
          <div><h1 className="text-2xl font-bold tracking-tight text-slate-900">Asset Hub</h1><p className="mt-1 text-sm text-slate-500">Sign in to manage your assets</p></div>
          <TextInput label="Username" value={username} onChange={setUsername} />
          <label className="grid gap-2">
            <span className="text-xs font-semibold text-slate-600">Password</span>
            <input className={inputClass} type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="current-password" />
          </label>
          {authError && <p className="rounded-lg border border-red-200 bg-red-50 p-3 text-sm text-red-700">{authError}</p>}
          <button className={primaryButtonClass} type="submit">Sign in</button>
        </form>
      </main>
    );
  }
  return <>{children({ user, initialDirectory: user.home_directory, logout })}</>;
}
