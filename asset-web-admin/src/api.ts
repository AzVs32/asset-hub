import { parseApiResponse } from "./apiResponse.js";

export const apiBase = import.meta.env.VITE_API_BASE_URL || "/api";

export async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${apiBase}${path}`, { credentials: "include", ...init });
  return (await parseApiResponse(response)) as T;
}
