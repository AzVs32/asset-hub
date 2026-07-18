export type UserRole = "administrator" | "member";
export type UserStatus = "active" | "disabled";

export interface CurrentUser {
  id: string;
  username: string;
  role: UserRole;
  workspaceDirectory: string;
  isAdmin: boolean;
}

export interface ManagedUser {
  id: string;
  username: string;
  role: UserRole;
  status: UserStatus;
  workspaceDirectory: string;
}
