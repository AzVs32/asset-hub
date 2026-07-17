export async function parseApiResponse(response: Response): Promise<unknown> {
  const text = await response.text();
  let data: unknown = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      throw new Error(`API returned invalid JSON (${response.status})`);
    }
  }
  if (!response.ok) {
    const error = data && typeof data === "object" && "error" in data
      ? String(data.error)
      : `${response.status} ${response.statusText}`;
    throw new Error(error);
  }
  return data;
}

