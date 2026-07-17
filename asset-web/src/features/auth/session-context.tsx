import React from "react";
import type { CurrentUser } from "@/domain/auth";

const SessionContext = React.createContext<CurrentUser | null>(null);

export function SessionProvider({
  user,
  children,
}: {
  user: CurrentUser;
  children: React.ReactNode;
}) {
  return <SessionContext.Provider value={user}>{children}</SessionContext.Provider>;
}

export function useSession(): CurrentUser {
  const user = React.useContext(SessionContext);
  if (!user) throw new Error("SessionProvider is missing");
  return user;
}
