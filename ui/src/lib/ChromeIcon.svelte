<script lang="ts">
  export type ChromeIconName =
    | "menu"
    | "clock"
    | "brain"
    | "settings"
    | "wrench"
    | "alert"
    | "panel"
    | "bell"
    | "plus"
    | "arrowDown";

  export let name: ChromeIconName;
  export let size = 16;
</script>

<!-- One stroke weight for the top bar. Filled marks read heavier and in more colors. -->
<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.75"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if name === "menu"}
    <path d="M4 7h16M4 12h16M4 17h16" />
  {:else if name === "clock"}
    <circle cx="12" cy="12" r="8" />
    <path d="M12 8v4l2.5 1.5" />
  {:else if name === "brain"}
    <path d="M12 5a3 3 0 1 0-5.997.125 4 4 0 0 0-2.526 5.77 4 4 0 0 0 .556 6.588A4 4 0 1 0 12 18Z" />
    <path d="M12 5a3 3 0 1 1 5.997.125 4 4 0 0 1 2.526 5.77 4 4 0 0 1-.556 6.588A4 4 0 1 1 12 18Z" />
    <path d="M15 13a4.5 4.5 0 0 1-3-4 4.5 4.5 0 0 1-3 4" />
    <path d="M12 18v3M8 15.5c-.5.8-1.2 1.4-2 1.8M16 15.5c.5.8 1.2 1.4 2 1.8" />
  {:else if name === "settings"}
    <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
    <circle cx="12" cy="12" r="3" />
  {:else if name === "wrench"}
    <path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z" />
  {:else if name === "alert"}
    <path d="M12 4.2 20 18.2H4L12 4.2z" />
    <path d="M12 9.2v4" />
    <path d="M12 16.4h.01" />
  {:else if name === "panel"}
    <rect x="3.5" y="4.5" width="17" height="15" rx="2" />
    <path d="M9 4.5v15" />
  {:else if name === "bell"}
    <path d="M6.2 16.5h11.6l-1-1.6a6 6 0 0 1-.8-3V9.6a4 4 0 0 0-8 0v2.3a6 6 0 0 1-.8 3l-1 1.6z" />
    <path d="M10 16.8a2 2 0 0 0 4 0" />
  {:else if name === "plus"}
    <path d="M12 5v14M5 12h14" />
  {:else if name === "arrowDown"}
    <path d="M12 5v14M7 14l5 5 5-5" />
  {/if}
</svg>

<style>
  svg {
    flex: none;
  }
</style>
