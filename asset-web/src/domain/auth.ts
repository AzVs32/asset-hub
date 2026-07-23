export type UserRole = "administrator" | "member";
export type UserStatus = "active" | "disabled";

export interface CurrentUser {
  id: string;
  username: string;
  role: UserRole;
  isAdmin: boolean;
}

export interface ManagedUser {
  id: string;
  username: string;
  role: UserRole;
  status: UserStatus;
  workspaceDirectory: string;
}
