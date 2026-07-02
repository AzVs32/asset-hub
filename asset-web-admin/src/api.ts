export const apiBase = import.meta.env.VITE_API_BASE_URL || "/api";

export async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${apiBase}${path}`, init);
  const text = await response.text();
  const data = text ? JSON.parse(text) : null;

  if (!response.ok) {
    throw new Error(data?.error ?? `${response.status} ${response.statusText}`);
  }

  return data as T;
}



