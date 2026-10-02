export interface Tab {
  id: string;
  kind: "harness" | "browser";
  title: string;
  badge?: string;
  badgeColor?: string;
  sessionId?: string | null;
  url?: string;
  active?: boolean;
  pinned?: boolean;
}
