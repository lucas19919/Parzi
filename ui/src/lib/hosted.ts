import { writable } from "svelte/store";
import { readJSON, writeJSON } from "./storage.ts";

export interface HostedSite {
  sessionId: string;
  url: string;
  title: string;
  time: number;
}

const STORAGE_KEY = "parzi.hosted";

function loadInitial(): Record<string, HostedSite> {
  const list = readJSON<HostedSite[]>(STORAGE_KEY, [], Array.isArray);
  const out: Record<string, HostedSite> = {};
  for (const s of list) {
    if (s && s.sessionId && s.url) out[s.sessionId] = s;
  }
  return out;
}

function persist(sites: Record<string, HostedSite>) {
  writeJSON(STORAGE_KEY, Object.values(sites).slice(-20));
}

export const hostedSites = writable<Record<string, HostedSite>>(loadInitial());

export function setHosted(sessionId: string, url: string, title: string) {
  if (!sessionId || !url) return;
  hostedSites.update((sites) => {
    const next = { ...sites, [sessionId]: { sessionId, url, title: title || hostLabel(url), time: Date.now() } };
    persist(next);
    return next;
  });
}

export function clearHosted(sessionId: string) {
  hostedSites.update((sites) => {
    if (!sites[sessionId]) return sites;
    const next = { ...sites };
    delete next[sessionId];
    persist(next);
    return next;
  });
}

export function hostLabel(url: string): string {
  try {
    const u = new URL(url);
    return u.port ? `${u.hostname}:${u.port}` : u.hostname;
  } catch {
    return url.slice(0, 40);
  }
}
