# Changelog

What changed, in plain words, newest first. Add a line under **Unreleased**
with each change people would notice. A dev build shows the Unreleased list;
a full release moves it under its version. See `docs/releasing.md`.

## Unreleased

## 1.4.1 (2026-11-13)

- Files live in the side bar now: one button, one bar. A Files card sits with the others (search, changed files, the folder as a tree), and opening a file, from the card, the thread or an agent, grows the side bar a second column beside the cards to read it in, resizable, with copy, save, show in folder and close. Closing it folds the bar back; the thread is never squeezed into a third column.
- One card look everywhere: Settings, Appearance, Cards (Clear, Glass or Solid) now styles the whole app, not just Home: the session frame, the side cards, the composer, the open tab, the cards in the thread (todo list, plan, tests, tasks, questions, suggestions, artifacts) and the menus. Clear is an outline, Glass a frosted tint with no outline, Solid the panel colour with an outline, with or without a wallpaper.
- You can follow an agent without opening anything: while it works, the thread shows the last thing it said or thought and its latest steps as they run, with a spinner, a tick or a cross and how long each took. It folds into "Worked for…" when the turn ends.
- Subagents show where you are working: the ones still running appear in the thread with the step each is on (click to open), even after the parent's own turn ends, and the Running card says what each is doing, or what it reported when it finished.
- Notifications are much quieter. Settings, General, Notifications sets how much Parzi tells you: Everything, When needed (the default: approvals, questions, requests and problems) or Nothing, plus a sound only when an agent needs you. Finished runs go quietly to the Notifications card; subagents no longer announce finishing or every spawn; a stop you asked for is not reported as an error; an approval's toast goes once you answer it; a repeated notice (a retry, a hook warning) shows once in ten minutes; a failing remote sync is said once, not every two minutes; and no toast pops up while the window is in the background.
- Replies stream smoothly: only the paragraph, list, table or code block being written changes, so earlier text no longer flickers or loses your selection while the agent writes. Text comes in at an even pace, a long code block stays open until it is finished, words after a card start a new paragraph, scrolling up mid-reply lets go of the bottom at once, and the end of a turn no longer shifts or dims the reply.
- The releases page keeps itself tidy: each release removes the older dev builds and drafts left by a release that never finished. Actions, Prune releases runs it by hand.
- The composer's menus (model, effort, permissions, project, branch) open the same way every time: just above the button, lined up with it, and below only when the window is too short. The model menu no longer spills out past the composer's left edge.
- On Home over the wallpaper with glass cards, the composer's menus (model, effort, project, branch) are glass too, instead of solid panels with an outline.
- Going from Home to a session and back eases instead of cutting: the wallpaper dissolves over half a second, and the glass cards and tabs turn solid, with their outlines fading in, over the same time.
- Light themes with the wallpaper filling the window show Home over a dimmed picture with white title bar text, and the cards and composer as light frosted glass with dark text, the same over any image. With the wallpaper as a band, light themes show it in its own colours, with no white veil.
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
- The side column is an inbox now: Environment first, then Inbox, then the plan. Cards swap by dragging, and the inbox sorts unread first with an Open button per row, dismiss, mark-all-read and an all-caught-up empty state.
- Plan items show what they involve: a chevron per item reveals its detail, dependencies, branch and note, with a one-line teaser when shut.
- The composer's file and plan pills and the queued-message rows follow the card look (Clear, Glass, Solid) like everything else.
- Agents can sleep until background work finishes: the `wait` tool watches shells, sessions or a plain delay and wakes with the result, instead of polling.
- New Google models arrive on their own: the Antigravity server moves to 1.4.0, a day-old cached model list quietly re-learns in the background, and Settings, Providers offers an Update button when your server is older than the pin.
- Opening a page in the system browser works for http pages too, such as local dev servers — not just https.

## 1.4.0 (2026-11-07)

- Agents can use anything in Parzi's window, the way you do: open tabs and sessions, pick the model and effort, open the side column and the file card, change settings and the theme. Named actions cover the common things, and every other button, switch and field is reachable too. Anything that deletes, removes, resets or signs out still waits for your yes.
- Ask an agent to change the look ("make it graphite", "turn on glass cards") and it does it right away instead of sending you to Settings.
- Charts animate: bars grow, lines draw on, numbers count up and rows rise in the first time a chart scrolls into view, and an updated chart eases to its new values. Reduced motion shows them still.
- The Home wallpaper can fill the whole window, and the cards over it can be clear, glass or solid (Settings, Appearance, Wallpaper).
- An accepted plan opens in the thread. The side column's plan card lists the plan's items, and steps aside while the plan is open in the thread.
- While an agent works, what it says between steps folds into its working instead of reading as its answer.

Earlier releases: see the [releases page](https://github.com/lucas19919/Parzi/releases).
