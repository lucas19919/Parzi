// The suggestions under an address box: the composer's web mode and a page's
// address bar both use this, so they complete, suggest and take keys the same
// way. It knows nothing about Svelte components or the stores that hold
// history; the box hands it what it needs.
import { writable, type Readable } from "svelte/store";
import { boxText, completeAddress, firstRows, looksLikeUrl, mergeRows, pageRows, phraseRows, type Suggestion } from "./suggest.ts";

export interface AddressSources {
  visits: { url: string; title: string; at: number; count: number }[];
  marks: { url: string; title: string }[];
}

export interface AddressOptions {
  /** History, bookmarks and pins as they are now. */
  sources: () => AddressSources;
  /** Search phrases for what was typed; may fail. */
  phrases: (typed: string) => Promise<string[]>;
  /** Whether the box still wants suggestions when late phrases arrive. */
  live: () => boolean;
  /** How long typing has to pause before phrases are asked for. */
  delay?: number;
}

export interface AddressState {
  rows: Suggestion[];
  active: number;
  typed: string;
}

/** What the box shows after a keystroke, and the completed part to select. */
export interface Shown {
  text: string;
  select: [number, number] | null;
}

export interface AddressSuggest extends Readable<AddressState> {
  /** Take what was typed. `inserting` is true for typing at the end of the box,
   *  the only time the rest of a known address is filled in. */
  type(value: string, inserting: boolean): Shown;
  /** Move the highlight; returns the text the box shows, or null with no list. */
  move(step: 1 | -1): string | null;
  /** Point at a row, as the mouse does. */
  hover(index: number): void;
  /** Close an open list; returns what was typed, or null if nothing was open. */
  escape(): string | null;
  /** The highlighted row, if any. */
  chosen(): Suggestion | null;
  /** Close the list and forget what was typed. */
  reset(): void;
}

const EMPTY: AddressState = { rows: [], active: -1, typed: "" };

export function addressSuggest(opts: AddressOptions): AddressSuggest {
  const state = writable<AddressState>(EMPTY);
  let now: AddressState = EMPTY;
  state.subscribe((s) => (now = s));
  let seq = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const stop = () => {
    seq++;
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  };

  return {
    subscribe: state.subscribe,

    type(value, inserting) {
      stop();
      const { visits, marks } = opts.sources();
      const full = inserting ? completeAddress(value, visits, marks) : "";
      const completed = full ? value + full.slice(value.length) : "";
      const local = () => mergeRows(firstRows(value, completed), pageRows(value, visits, marks));
      state.set({ rows: value.trim() ? local() : [], active: -1, typed: value });
      if (value.trim() && !looksLikeUrl(value)) {
        const mine = seq;
        timer = setTimeout(async () => {
          const phrases = await opts.phrases(value).catch(() => [] as string[]);
          if (mine !== seq || !opts.live() || now.typed !== value) return;
          state.update((s) => ({ ...s, rows: mergeRows(firstRows(value, completed), phraseRows(value, phrases), pageRows(value, visits, marks)) }));
        }, opts.delay ?? 120);
      }
      return { text: completed || value, select: completed ? [value.length, completed.length] : null };
    },

    move(step) {
      const { rows, active, typed } = now;
      if (!rows.length) return null;
      const next = active + step < -1 ? rows.length - 1 : active + step >= rows.length ? -1 : active + step;
      state.set({ ...now, active: next });
      return next >= 0 ? boxText(rows[next]) : typed;
    },

    hover(index) {
      if (index >= -1 && index < now.rows.length) state.set({ ...now, active: index });
    },

    escape() {
      if (!now.rows.length) return null;
      stop();
      const typed = now.typed;
      state.set({ ...now, rows: [], active: -1 });
      return typed;
    },

    chosen() {
      return now.active >= 0 ? (now.rows[now.active] ?? null) : null;
    },

    reset() {
      stop();
      state.set(EMPTY);
    },
  };
}
