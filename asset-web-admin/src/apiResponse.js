export async function parseApiResponse(response) {
  const text = await response.text();
  let data = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      throw new Error(`API returned invalid JSON (${response.status})`);
    }
  }
  if (!response.ok) {
    throw new Error(data?.error ?? `${response.status} ${response.statusText}`);
  }
  return data;
}
