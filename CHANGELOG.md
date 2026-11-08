# Changelog

What changed, in plain words, newest first. Add a line under **Unreleased**
with each change people would notice. A dev build shows the Unreleased list;
a full release moves it under its version. See `docs/releasing.md`.

## Unreleased

- Light themes show the wallpaper in its own colours: no white veil or fade over it, and the dither no longer greys the picture. Tabs, title bar buttons and the line above the composer read the picture behind them and turn light where it is dark, dark where it is light.
- Agents can put a file in front of you: a plan, a doc they wrote or a project file opens in the File card beside the conversation, markdown rendered (the ui_show_file tool).
- The File card browses the session's project as a tree of its folders, with the files changed in the working tree on top and a search across the whole project; click a file to read it and the arrow to go back. Your projects' folders work before any session has run in them, and opening a file from anywhere on your computer works again.
- Glass tabs use the same glass as the cards.
- Buttons on glass cards are a faint tint with no outline, one surface with the glass instead of grey on grey.
- Glass cards, the glass composer and glass tabs over the wallpaper have no outline: the frost is the edge. Clear cards keep theirs.
- Updates show up on their own: Parzi looks every half hour and when you come back to the window, retries soon if a look or a download fails, and keeps a downloaded update ready instead of fetching it again.
- Agents on your server live in Settings, Providers, next to this computer's: pick the server at the top and each agent shows as the same card, with its status, usage and an Install or Sign in button.
- The File card turns glass (or solid) over the wallpaper like the other cards, instead of staying see-through.
- Rounded things inside a box now nest with it: a picked segment, menu and picker rows, a tab's close button, plan rows and theme swatches sit one even inset from every edge, with a corner that runs parallel to the box's.
- Toggles are one height everywhere, and the picked segment sits the same distance from each edge.
- Over the wallpaper, the open tab, a hovered tab and the side-column toggle are frosted glass (solid when cards are solid), so they no longer vanish into the picture.
- The wallpaper's grain is now a dither you can tune: strength and dot size, under Settings, Appearance, Wallpaper. Agents can set them too.
- Dev builds: turn them on in Settings, General, and Parzi updates to pre-releases as soon as they are out. Off, it stays on full releases.
- Phones are coming: `parzi serve` can open a gateway for paired devices (`parzi gateway on`, `parzi devices add`), the first step of Parzi mobile.

## 1.4.0 (2026-11-07)

- Agents can use anything in Parzi's window, the way you do: open tabs and sessions, pick the model and effort, open the side column and the file card, change settings and the theme. Named actions cover the common things, and every other button, switch and field is reachable too. Anything that deletes, removes, resets or signs out still waits for your yes.
- Ask an agent to change the look ("make it graphite", "turn on glass cards") and it does it right away instead of sending you to Settings.
- Charts animate: bars grow, lines draw on, numbers count up and rows rise in the first time a chart scrolls into view, and an updated chart eases to its new values. Reduced motion shows them still.
- The Home wallpaper can fill the whole window, and the cards over it can be clear, glass or solid (Settings, Appearance, Wallpaper).
- An accepted plan opens in the thread. The side column's plan card lists the plan's items, and steps aside while the plan is open in the thread.
- While an agent works, what it says between steps folds into its working instead of reading as its answer.

Earlier releases: see the [releases page](https://github.com/lucas19919/Parzi/releases).
