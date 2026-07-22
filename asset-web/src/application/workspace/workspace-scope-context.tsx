import React from "react";
import type { CurrentUser } from "@/domain/auth";
import { createWorkspaceScope, type WorkspaceScope } from "./workspace-scope";

const WorkspaceScopeContext = React.createContext<WorkspaceScope | null>(null);

export function WorkspaceScopeProvider({
  user,
  children,
}: {
  user: CurrentUser;
  children: React.ReactNode;
}) {
  const scope = React.useMemo(() => createWorkspaceScope(user), [user]);
  return <WorkspaceScopeContext.Provider value={scope}>{children}</WorkspaceScopeContext.Provider>;
}

export function useWorkspaceScope(): WorkspaceScope {
  const scope = React.useContext(WorkspaceScopeContext);
  if (!scope) throw new Error("WorkspaceScopeProvider is missing");
  return scope;
}
