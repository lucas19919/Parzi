# Parzi Overhaul & Polish — Changes Summary

This document summarizes the full architectural, frontend, and design overhaul performed on Parzi.

---

## 1. Window Chrome & Dragging Fix

### Problem
In Tauri v2, window manipulation (`minimize`, `maximize`, `close`, `start_dragging`) is gated behind strict capability permissions. The app had `decorations: false` (frameless), but [`capabilities/default.json`](src-tauri/capabilities/default.json) lacked window permissions, causing all window control buttons and dragging to silently fail.

### Changes
* **Tauri Capabilities**: Updated [`src-tauri/capabilities/default.json`](src-tauri/capabilities/default.json) with:
  - `core:window:default`
  - `core:window:allow-start-dragging`
  - `core:window:allow-minimize`
  - `core:window:allow-maximize`
  - `core:window:allow-unmaximize`
  - `core:window:allow-toggle-maximize`
  - `core:window:allow-close`
  - `core:window:allow-set-fullscreen`
  - `core:window:allow-is-fullscreen`
* **Native Rust Window Commands**: Added IPC commands to [`src-tauri/src/main.rs`](src-tauri/src/main.rs) (`window_minimize`, `window_maximize`, `window_close`, `window_start_dragging`) to guarantee window controls bypass any frontend permission mismatches.
* **Modular Titlebar**: Created [`ui/src/lib/Titlebar.svelte`](ui/src/lib/Titlebar.svelte) with working drag zone, workspace/task breadcrumb, and responsive window control buttons.

---

## 2. Workspaces: Projects $\rightarrow$ Tasks $\rightarrow$ Lanes

### Concept
Replaced the flat, unstructured chat list with a structured **Project $\rightarrow$ Task $\rightarrow$ Lane** hierarchy:
* **Project (Workspace)**: A repository root on disk (e.g., `acme`).
* **Task**: A concrete goal or feature (e.g., `building axiom`), with a target subfolder (e.g., `crates/axiom`) or root directory.
* **Plan Mode**: You define the high-level objective once, outline an architectural plan, and orchestrate subagent **Lanes**.
* **Lanes**: Focused subagent execution threads (e.g. `core`, `ui`, `tests`) running with the task's objective and working directory.

### Core & Backend Changes
* Added [`TaskConfig`](crates/parzi-core/src/lanes.rs#L166-L180) to `parzi-core`:
  ```rust
  pub struct TaskConfig {
      pub id: String,
      pub title: String,
      pub project: String,
      pub subfolder: Option<String>,
      pub objective: String,
      pub plan_md: String,
      pub status: String,
      pub lanes: Vec<String>,
      pub created_at: String,
  }
  ```
* Implemented persistence under `~/.parzi/projects/<project>/tasks/<task_id>/task.json`.
* Added `create_task`, `list_tasks`, `save_task`, and `delete_task` in `parzi-core` and exposed them as Tauri IPC commands in [`src-tauri/src/main.rs`](src-tauri/src/main.rs) and [`ui/src/lib/api.ts`](ui/src/lib/api.ts).
* Added auto-creation of subfolders inside the project root when a new task is initialized.

---

## 3. UI/UX Modularization & Reference Aesthetic

### Problem
[`ui/src/App.svelte`](ui/src/App.svelte) had ballooned to 1,150 lines, containing placeholder clutter (*"Pull requests soon"*, *"Automations soon"*), duplicated cards, and hardcoded logic.

### Changes
* **Deconstructed into focused components**:
  - [`Titlebar.svelte`](ui/src/lib/Titlebar.svelte): Frameless drag bar + window actions.
  - [`Sidebar.svelte`](ui/src/lib/Sidebar.svelte): Project switcher, task tree with child lanes, clean footer with git branch and live runs.
  - [`TaskPlanningView.svelte`](ui/src/lib/TaskPlanningView.svelte): Task objective prompt editor, plan markdown editor, and subagent lane orchestrator.
  - [`Omnibar.svelte`](ui/src/lib/Omnibar.svelte): Minimalist floating glass bar.
  - [`Thread.svelte`](ui/src/lib/Thread.svelte): Chronological timeline.
  - [`App.svelte`](ui/src/App.svelte): Streamlined down to a clean view coordinator.
* **Eliminated Clutter**:
  - Removed dummy/disabled buttons (*"Pull requests soon"*, *"Automations soon"*).
  - Removed duplicate workspace cards in the sidebar.
  - Removed hero text (`"What are we building tonight?"`) and suggestion chips (`"Explain this codebase"`, etc.) for an ultra-clean, distraction-free stage.

---

## 4. Omnibar & Micro-Interactions

### Changes
* **Transitions.dev Segmented Control (Effort / Difficulty)**:
  - Built an animated segmented slider for **Low** | **Med** | **High**.
  - Features an animated sliding indicator pill driven by `cubic-bezier(0.16, 1, 0.3, 1)` spring physics that smoothly glides behind the active selection.
* **T3-Style Anchored Model Popover**:
  - Replaced the full-screen modal with a sleek floating popover anchored directly above the model pill.
  - Includes instant live search, provider logos, context window tags (`200k`, `128k`), selection checkmarks, and click-outside dismissal.
* **Send Button Refinement**:
  - Removed the blurry, fluorescent outer neon glow.
  - Replaced with a crisp tactile jewel button with top edge lighting, clean 7px radius, and subtle shadow.
* **Removed Plus Icon**:
  - Removed the redundant `+` button from the composer footer.
* **Refined Curvature**:
  - Polished rounding across all elements (`14px` composer, `12px` popover, `7px` pills) for a cohesive, modern feel.

---

## 5. Chronological Tool & Chat Timeline

### Problem
[`ui/src/lib/Thread.svelte`](ui/src/lib/Thread.svelte) previously collected all tool executions and dumped them into a flat pile at the very bottom of the conversation thread, out of order.

### Changes
* Implemented a unified `chronologicalItems` pipeline.
* User messages, reasoning thoughts, tool calls, live running states, tool outputs, and assistant responses now render in the exact chronological sequence in which they occurred.

---

## 6. Subscriptions & OAuth vs. API Key Router

### Changes
* In [`ui/src/lib/Settings.svelte`](ui/src/lib/Settings.svelte), divided the Models tab into:
  1. **Subscriptions & Ported OAuth**: Automatically detects local developer sessions for Codex (`~/.codex/auth.json`), Claude Code (`~/.claude/.credentials.json`), OpenCode, and Google Antigravity with live connection indicators (`✓ Connected` or hint to run CLI login).
  2. **Direct API Keys**: Keyring-backed API key storage for OpenAI, Anthropic, xAI, OpenRouter, Groq, Mistral, and Ollama.

---

## 7. Verification & Build Results

| Check / Build Target | Command | Result |
| :--- | :--- | :--- |
| **Rust Workspace Tests** | `cargo test --workspace` | **19 / 19 passed** (0 failed) |
| **Frontend UI Build** | `npm run build` in `ui/` | **Built cleanly** (0 errors) |
| **Tauri Manifest Check** | `cargo check --manifest-path src-tauri/Cargo.toml` | **Clean** (0 errors) |
| **Tauri Binary Compilation** | `cargo build --manifest-path src-tauri/Cargo.toml` | **`parzi-app.exe` compiled successfully** |
