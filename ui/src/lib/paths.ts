// Shared path helpers (attachments, project folders).
export function sameDir(a: string, b: string): boolean {
  const norm = (p: string) => p.replace(/[/\\]+$/, "").replace(/\\/g, "/").toLowerCase();
  return !!a && !!b && norm(a) === norm(b);
}

export function relativeTo(path: string, root: string): string {
  const clean = root.replace(/[/\\]+$/, "");
  if (clean && (path.startsWith(`${clean}\\`) || path.startsWith(`${clean}/`))) {
    return path.slice(clean.length + 1);
  }
  return path;
}

export function shortAttachment(name: string): string {
  const base = name.split(/[/\\]/).pop() ?? name;
  return base.length > 22 ? `${base.slice(0, 22)}…` : base;
}
