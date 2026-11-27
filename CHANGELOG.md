# Changelog

What changed, in plain words, newest first. Add a line under **Unreleased**
with each change people would notice. A dev build shows the Unreleased list;
a full release moves it under its version. See `docs/releasing.md`.

## Unreleased

- Settings, Providers is plainer: one type for every row, a Billing row instead of a line repeating the status, and limits named Session, Weekly, Monthly (or Quota) without the model in front.
- The side column shows only cards with something in them: Context appears once the session has used some, Running once something runs.
- Antigravity's Gemini models show their 5-hour quota in the ring, read from the same model list the agent uses. Settings shows only the limits of models an agent offers, so Antigravity no longer lists a Claude/GPT quota you don't use. The ring is hover only; clicking it no longer opens Settings.
- The open tab, and a session's stack of tabs, have the cards' glass edge again, on Home over the wallpaper too. The Environment card shows a branch only for a git folder.
- Plan limits are a ring in the composer, for the model you're using: it fills with the session (5-hour) window, and hovering it shows every window that applies (weekly, monthly, model-only). The Usage card in the side column is gone. A limit that belongs to other models (Claude's Opus-only week, Antigravity's Claude/GPT quota) no longer shows on yours.
- Antigravity lists each model once (Gemini 4 Neon, Carbon, Argon…) and the effort picker chooses the variant (Low to Extra high), instead of a row per effort.
- Files are never opened from network or device paths (`\\host\share`), which on Windows would send your sign-in to that host; any local path still opens.
- Tests clear the folders earlier runs left in the temp folder instead of piling them up.
- Pages and files open in tabs stacked under their session again, instead of full screen over it: a pin, a link in the thread, a web search or a file an agent shows lands in the session's stack, and comes back after a restart (pages). The pins above the composer stay. The side file reader beside the thread is gone; files open in their own tab.
- The Usage card shows Grok, OpenCode Go and Antigravity's plan limits too, as their own /usage does: Grok's weekly (or monthly) credits, OpenCode Go's 5-hour, weekly and monthly spend, and Antigravity's 5-hour and weekly quota for each model group. Each is read from the agent's own sign-in, which Parzi never changes.
- Remade the iPhone Duo browser viewer with real hardware proportions: landscape and portrait inner displays with a rotate toggle and hidden under-display camera, plus the closed outer display with its titanium spine, asymmetric corners, and right-side vertical Dynamic Island rail.
- Opening or revealing local files and folders now works from any path without workspace-folder restrictions.
- Added a branch and worktree picker in the Environment card to switch branches, jump between worktrees, or spin up a new worktree for a session.
- Polished session cards and markdown: proposed plans collapse long step lists with quick expand, questions support number-key shortcuts, Running cards label shell commands by intent, and links, math, and pins render more cleanly.
- Hardening and reliability fixes across the engine, providers, gateway, CLI, and UI: preserved project plans and ACP settings, tightened tool approval gates, secret path guards, OAuth URL validation, and image path checks, fixed Claude usage retries and child-exit handling, kept remote device unpairing/scope checks live, and fixed session switching, draft recovery, phone-frame fullscreen, markdown fences, artifact updates, and context/running cards.
- The thread is quieter: each turn is one line (Worked 2m · 12 steps, or Thinking… while it runs) that opens to its steps. Notices such as plan saved, compacted or stopped, and messages from other sessions, are in the Running card. Errors are one line with their buttons, timestamps and rewind show on hover, the plan checklist lives behind the Plan pin, and reply chips are smaller. The tidied display of your messages is gone.
- The session manager lists sessions only: drafts stay on the card that wrote them, and the filters above the list are your projects, not folders.
- The Usage card is your plan's limits, nothing else: Session and Weekly (and Monthly where the plan has one) with the percent and when it resets, dashes until read. On an API key it shows tokens and cost. Context shows the tokens used until the agent reports its window. The project's targets are one small chip (done of total) above the composer.
- The Usage card shows your plan's real limits: Claude's 5-hour and weekly windows as Claude Code's /usage shows them, and Codex's as its /status does, read when the card shows and every 5 minutes, not only when a turn happens to report them.
- A page shown as a phone looks like the phone: an iPhone 16 Pro with its Dynamic Island, titanium edge and buttons, or a Pixel 9 with its punch-hole camera, with a status bar and home indicator in the page's own colour. The page gets the screen between them, as in Safari or Chrome. The iPhone Duo can be shown open (its 7.6-inch inner screen) or closed (its 5.4-inch outer one).
- Agents can script your connectors: from a session's shell, `parzi connector tools github` prints the tools as typed signatures and `parzi connector call github <tool> '<json>'` runs one, with the same approvals as any connector call. A chain like "list my open PRs, keep the failing ones, comment on each" becomes one script instead of a turn per call, and the bulky results stay out of the conversation. It works without the parzi CLI installed: the app answers it.
- See a page as a phone: a page opened full screen over a session switches between laptop, iPhone and Android. The phones are emulated for real (their screen size and pixel density, their browser's user agent, and touch for the mouse), drawn in a plain frame and scaled to fit, so the site lays out and behaves as it does on the phone.
- Projects have a plan: a short goal and an ordered list of targets that agents keep with `project_plan`. The open targets show above the composer, with a dot on the one being worked on; work can be linked to a target when it is drafted or handed to a helper.
- A proposed plan no longer holds the agent in the middle of a tool call: the agent stops, the plan waits on the session (even across a restart), and Accept, Not now or your own words go to it as your next message. A plan's full document is kept with the session, so the Plan pin keeps working.
- A session's side column has four cards in a fixed order: Environment (move a stopped session to another project or folder, such as a worktree on another branch), Context, Usage (each limit of the agent in use and when it resets) and Running (what the agent did, step by step). Above the composer sit the session's pins (its dev servers and its plan) and the project's open targets. Links, pins and web searches open full screen over the session with the composer still there, instead of in a new tab.
- Antigravity turns no longer hang for up to 20 minutes after replying: Antigravity reports its file reads finished only when the whole prompt ends, and Parzi no longer waits on those once the reply is written.
- Agents carry less: big connectors (GitHub, IsarOS) are listed by name until an agent asks for their tools, Parzi's own instructions are a third of their old length, and a few duplicate window tools are gone. Every agent now gets the repository's instructions, whether the repo keeps them in CLAUDE.md or AGENTS.md.
- Switching agents and back resumes the first agent's own session, and it hears only what happened while it was away. ACP agents that can load a session (but not resume one) now pick up where they left off.
- Retrying with another model resends the files the message carried.
- Parzi is smaller. Gone: the ad blocker, Bitwarden passwords, Parzi's own download handling (pages download as the browser does), the site permission bar (pages ask with the browser's own prompt), bookmark and history import with its setup step, the notifications bell and its settings, and the Files, Hosts and Changes cards. A session's tab shows how it stands instead: a spinner while it works, an amber dot when it needs you, and a green or red dot when it finished or failed and you haven't opened it yet.
- Removed the bottom fade overlays on glass code blocks and folded artifact cards.
- Bare localhost, IP, and local/intranet addresses with a port or path now linkify and open directly in a Parzi tab.
- Long threads stay snappy: sessions with large histories render the recent tail first with a "Show earlier messages" button, and tab-group corners align cleanly in the tab bar.
- Agents name sessions themselves with `session_rename` instead of running a separate formatting model; removed the Formatting agent setting, the tidied display of your messages and `ui_host_site`.
- ACP sessions that finish replying and report usage before going silent now settle cleanly instead of hanging.
- A proposed plan reads like a plan: what and why at full size, each step with what it involves, a link to the agent's full plan document, and one Accept. To change it, type what you want in the composer; it goes straight to the agent.
- Questions are calm choice cards: each answer is a tile you click to answer at once (several can be picked and sent together when the agent allows it), "Something else…" takes your own words, and Skip sits quietly by the title. No radio buttons or Submit.
- Every card (the session, the side cards, the composer, the plan, questions, menus, tooltips) shows its rounded outline in one faint hairline, so its edge reads over a dark picture and the session's corner no longer looks like a stray strip at the top left. The dark fade under the composer is gone. Working spinners are one quiet tone everywhere.
- Your message shows the moment you send it and stays: it no longer vanished (leaving only "Thinking") when the agent took a moment to start.
- Math is big enough to read: fractions, sums and integrals in a sentence are drawn full size, all math is a touch larger, and a tidied message puts an equation with fractions on its own line.
- Only pictures a tool made show in the thread (a screenshot, a diagram): a path that merely appears in a listing or a search no longer pulls in an unrelated image, and a picture that can't load is left out instead of announced.
- The queue is one calm card: "Queued · 3 · sends when the agent is done", click a message to edit it, drag to reorder, actions on hover.
- A stopped turn says it once: "Stopped after 12s · 3 steps", in the same quiet line as "Worked for…". The wallpaper as a band stays a band behind sessions too.
- Switching sessions is smooth: the thread fades out, the next one loads at its bottom out of sight and fades in, instead of a jump. The composer is the same glass on Home and in a session.
- Each session has its own plan: the agent's checklist is kept on the session, survives a restart or a switch to another agent, and shows in the thread as one Plan card; the session manager shows "3 of 7". For a bigger piece of work an agent can ask first (Use this plan / Not now). Project-wide plans, plan items and quote chips are gone.
- Work for later is a draft: an agent writes it down (a title, a brief and a rough plan) instead of a suggestion card, it waits under Drafts in the session manager, and Start runs it on the same session. Old project plans turn into drafts once, on the first start after this update (the old note is kept as plan.archived.md).
- Switching to another agent keeps the thread: at a usage limit, Switch model picks another model and resends your last message on the same session, and the new agent gets the conversation so far instead of a fresh, empty session.
- A rebuilt session manager: Needs you and Working first (with what each agent is doing), then Drafts, then sessions by day. Type to find, arrows and Enter to open, project chips to filter.
- One look across the app: everything is the frosted glass of Home (cards, the composer, menus, tooltips, toasts), buttons are one filled style with no outlines, and the dimmed picture sits behind sessions too. The Cards setting (Clear, Glass, Solid) is gone. The usage-limit card and the Plan card are calmer, and question cards lose their divider lines.

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
