import { WindowMessenger } from "penpal";

const pluginAssetPrefix = /^\/plugins\/[a-z0-9._-]+\//;

export function createPluginFrameMessenger(remoteWindow: Window): WindowMessenger {
  return new WindowMessenger({
    remoteWindow,
    // The sandbox intentionally creates an opaque origin. Penpal still binds this exact Window.
    allowedOrigins: ["*"],
  });
}

export function pluginFrameUrl(
  value: string,
  resolveUrl: (url: string) => string | null,
): string | null {
  const [path] = value.split(/[?#]/, 1);
  if (!path || !pluginAssetPrefix.test(path) || hasUnsafePathSegment(path)) return null;
  return resolveUrl(value);
}

function hasUnsafePathSegment(path: string): boolean {
  try {
    return path.split("/").some((segment) => {
      const decoded = decodeURIComponent(segment);
      return decoded === "." || decoded === ".." || decoded.includes("/") || decoded.includes("\\");
    });
  } catch {
    return true;
  }
}
