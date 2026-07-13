function normalize(path) {
  return path.split("/").filter(Boolean).join("/");
}

function contains(root, directory) {
  return root === "" || directory === root || directory.startsWith(`${root}/`);
}

export function directoryBreadcrumbs(directory, rootDirectory, rootLabel) {
  const current = normalize(directory);
  const root = normalize(rootDirectory);
  if (!contains(root, current)) {
    const parts = current.split("/");
    return [{ label: parts[parts.length - 1] || rootLabel, path: current }];
  }

  const crumbs = [{ label: rootLabel, path: root }];
  const relative = root === "" ? current : current.slice(root.length).replace(/^\//, "");
  const parts = relative.split("/").filter(Boolean);
  parts.forEach((label, index) => {
    const suffix = parts.slice(0, index + 1).join("/");
    crumbs.push({ label, path: root ? `${root}/${suffix}` : suffix });
  });
  return crumbs;
}

export function parentDirectoryWithinRoot(directory, rootDirectory) {
  const current = normalize(directory);
  const root = normalize(rootDirectory);
  if (current === root || !contains(root, current)) return null;

  const parts = current.split("/");
  parts.pop();
  const parent = parts.join("/");
  return contains(root, parent) ? parent : null;
}
