import { writable } from "svelte/store";

/** Sessions whose task list sits in the session column instead of the thread. */
export const dockedTodos = writable<Set<string>>(new Set());

export function setTodoDocked(session: string, docked: boolean) {
  dockedTodos.update((s) => {
    const next = new Set(s);
    if (docked) next.add(session);
    else next.delete(session);
    return next;
  });
}
