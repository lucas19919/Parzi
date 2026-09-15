//! Composer state machine + thin egui view (Phase 2, M2).
//!
//! Ports the STRUCTURE of `ui/src/lib/Omnibar.svelte` — multi-line edit with
//! Enter-to-send, attachment chips, model/effort/permission pills, slash-@
//! menus — not its styling. The state machine is pure and unit-tested; [`show`]
//! is a thin `TextEdit` wrapper that only needs to compile, never to run in
//! tests (no GPU). Sending itself is the caller's job (`Orchestrator::send_to`
//! + `bridge.track_run`), so the view stays dumb: no control without a
//! handler (AUDIT U-2 — the handler is whoever calls [`ComposerState::submit`]).

pub mod menus;

use std::path::{Path, PathBuf};

/// Cap mirrors the web composer (`attachments.slice(0, 8)`).
pub const MAX_ATTACHMENTS: usize = 8;

/// Output-budget rung. [`Effort::as_str`] feeds `Orchestrator::spawn` /
/// `send_to` directly (the same ids `normalize_effort` accepts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Effort {
    Low,
    #[default]
    Medium,
    High,
    Extra,
    Ultra,
}

impl Effort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Extra => "extra",
            Self::Ultra => "ultra",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Extra => "Extra",
            Self::Ultra => "Ultra",
        }
    }

    /// Menu hint. Mirrors the Omnibar effort options.
    pub fn hint(self) -> &'static str {
        match self {
            Self::Low => "4k output",
            Self::Medium => "16k output",
            Self::High => "64k output",
            Self::Extra => "128k output",
            Self::Ultra => "256k output",
        }
    }

    /// Legacy `"med"` still resolves (old defaults, queued runs); anything
    /// unknown falls back to medium, like the orchestrator does.
    pub fn normalize(s: &str) -> Self {
        match s {
            "low" => Self::Low,
            "medium" | "med" => Self::Medium,
            "high" => Self::High,
            "extra" => Self::Extra,
            "ultra" => Self::Ultra,
            _ => Self::Medium,
        }
    }

    /// `/effort` cycles Low → … → Ultra → Low.
    pub fn cycle(self) -> Self {
        match self {
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::Extra,
            Self::Extra => Self::Ultra,
            Self::Ultra => Self::Low,
        }
    }
}

/// Compose mode. Mirrors the Omnibar MODES (chat/plan/build).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ComposerMode {
    #[default]
    Chat,
    Plan,
    Build,
}

impl ComposerMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Plan => "plan",
            Self::Build => "build",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Chat => "Chat",
            Self::Plan => "Plan",
            Self::Build => "Build",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Chat => "Ask and edit freely.",
            Self::Plan => "Plan only — no edits or commands.",
            Self::Build => "Ship it — full execution.",
        }
    }
}

/// Permission pill. Mirrors the Omnibar PERMS ids (default: full access,
/// same as the web composer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Permission {
    Supervised,
    Edits,
    Auto,
    #[default]
    Full,
}

impl Permission {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supervised => "supervised",
            Self::Edits => "edits",
            Self::Auto => "auto",
            Self::Full => "full",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Supervised => "Supervised",
            Self::Edits => "Auto-accept edits",
            Self::Auto => "Auto",
            Self::Full => "Full access",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Supervised => "Ask before commands and file changes.",
            Self::Edits => "Auto-approve edits, ask before other actions.",
            Self::Auto => "Routine actions auto-approved; the rest still ask.",
            Self::Full => "Allow commands and edits without prompts.",
        }
    }
}

/// Outcome of a key press inside the composer box. Pure: the egui view maps
/// `Key::Enter` / `Key::Escape` plus the Shift modifier onto [`key_filter`],
/// so the whole matrix is unit-testable with no GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutcome {
    Submit,
    Newline,
    Dismiss,
    Ignored,
}

/// Enter submits, Shift+Enter inserts a newline, Escape dismisses open menus,
/// Enter on an empty box does nothing. `enter`/`escape` are "was pressed this
/// frame", `shift` is "held", `has_text` is "the box holds non-blank text".
pub fn key_filter(enter: bool, escape: bool, shift: bool, has_text: bool) -> KeyOutcome {
    if escape {
        return KeyOutcome::Dismiss;
    }
    if !enter {
        return KeyOutcome::Ignored;
    }
    if shift {
        return KeyOutcome::Newline;
    }
    if has_text {
        KeyOutcome::Submit
    } else {
        KeyOutcome::Ignored
    }
}

/// A sendable draft: what `Orchestrator::send_to` needs for one user turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    pub text: String,
    pub attachments: Vec<PathBuf>,
    pub effort: Effort,
    pub mode: ComposerMode,
    pub permission: Permission,
}

/// Composer state: draft text, `@`-attached file chips, and the three pills.
#[derive(Debug, Clone, Default)]
pub struct ComposerState {
    pub text: String,
    pub attachments: Vec<PathBuf>,
    pub effort: Effort,
    pub mode: ComposerMode,
    pub permission: Permission,
}

impl ComposerState {
    pub fn new() -> Self {
        Self::default()
    }

    /// The send button is armed only for non-blank drafts (web parity).
    pub fn can_submit(&self) -> bool {
        !self.text.trim().is_empty()
    }

    /// Take the draft, clearing the box and chips (the web composer clears on
    /// send). `None` when the box is blank. Pills are kept.
    pub fn submit(&mut self) -> Option<Submission> {
        if !self.can_submit() {
            return None;
        }
        Some(Submission {
            text: std::mem::take(&mut self.text),
            attachments: std::mem::take(&mut self.attachments),
            effort: self.effort,
            mode: self.mode,
            permission: self.permission,
        })
    }

    /// Attach a file chip (`@` flow / file picker). Dedups and caps at
    /// [`MAX_ATTACHMENTS`]; returns false when the path was ignored.
    pub fn add_attachment(&mut self, path: PathBuf) -> bool {
        if self.attachments.contains(&path) || self.attachments.len() >= MAX_ATTACHMENTS {
            return false;
        }
        self.attachments.push(path);
        true
    }

    pub fn remove_attachment(&mut self, path: &Path) -> bool {
        let before = self.attachments.len();
        self.attachments.retain(|p| p != path);
        self.attachments.len() != before
    }

    pub fn clear_attachments(&mut self) {
        self.attachments.clear();
    }
}

/// What the view reports; sending itself stays with the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposerAction {
    pub edited: bool,
}

/// Thin multi-line box. IME comes free via winit/egui (PLAN §1 confirmed).
/// Enter-to-send is deliberately NOT read here: the caller maps
/// `ui.input(|i| (i.key_pressed(egui::Key::Enter), i.modifiers.shift))`
/// through [`key_filter`] so the rule stays unit-testable (see tests).
pub fn show(ui: &mut egui::Ui, state: &mut ComposerState) -> ComposerAction {
    let response = egui::TextEdit::multiline(&mut state.text)
        .hint_text("Ask anything, @ to mention, / for actions")
        .desired_rows(1)
        .desired_width(f32::INFINITY)
        .show(ui)
        .response;
    ComposerAction {
        edited: response.changed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_key_matrix() {
        use KeyOutcome::{Dismiss, Ignored, Newline, Submit};
        // Enter + text submits; Shift+Enter always breaks the line.
        assert_eq!(key_filter(true, false, false, true), Submit);
        assert_eq!(key_filter(true, false, true, true), Newline);
        assert_eq!(key_filter(true, false, true, false), Newline);
        // Enter on an empty box is a no-op (send button disarmed).
        assert_eq!(key_filter(true, false, false, false), Ignored);
        // Anything else is ignored…
        assert_eq!(key_filter(false, false, false, true), Ignored);
        assert_eq!(key_filter(false, false, true, true), Ignored);
        // …except Escape, which dismisses open menus.
        assert_eq!(key_filter(false, true, false, false), Dismiss);
        assert_eq!(key_filter(true, true, false, true), Dismiss);
    }

    #[test]
    fn effort_ids_round_trip() {
        assert_eq!(Effort::normalize("med"), Effort::Medium);
        assert_eq!(Effort::normalize("ultra"), Effort::Ultra);
        assert_eq!(Effort::normalize("bogus"), Effort::Medium);
        for e in [
            Effort::Low,
            Effort::Medium,
            Effort::High,
            Effort::Extra,
            Effort::Ultra,
        ] {
            assert_eq!(Effort::normalize(e.as_str()), e);
        }
    }

    #[test]
    fn effort_cycles_through_all_rungs() {
        assert_eq!(Effort::Low.cycle(), Effort::Medium);
        assert_eq!(Effort::Medium.cycle(), Effort::High);
        assert_eq!(Effort::High.cycle(), Effort::Extra);
        assert_eq!(Effort::Extra.cycle(), Effort::Ultra);
        assert_eq!(Effort::Ultra.cycle(), Effort::Low);
    }

    #[test]
    fn attachments_dedup_cap_and_remove() {
        let mut s = ComposerState::new();
        assert!(s.add_attachment(PathBuf::from("a.rs")));
        assert!(!s.add_attachment(PathBuf::from("a.rs"))); // dup
        for i in 0..MAX_ATTACHMENTS {
            s.add_attachment(PathBuf::from(format!("f{i}.rs")));
        }
        assert_eq!(s.attachments.len(), MAX_ATTACHMENTS);
        assert!(!s.add_attachment(PathBuf::from("overflow.rs"))); // capped
        assert!(s.remove_attachment(Path::new("a.rs")));
        assert!(!s.remove_attachment(Path::new("a.rs")));
        s.clear_attachments();
        assert!(s.attachments.is_empty());
    }

    #[test]
    fn submit_takes_the_draft_and_keeps_the_pills() {
        let mut s = ComposerState::new();
        s.text = "   ".into();
        assert!(!s.can_submit());
        assert_eq!(s.submit(), None);

        s.text = "fix the leak".into();
        s.add_attachment(PathBuf::from("leak.rs"));
        s.effort = Effort::High;
        s.mode = ComposerMode::Build;
        let sub = s.submit().expect("non-blank draft sends");
        assert_eq!(sub.text, "fix the leak");
        assert_eq!(sub.attachments, vec![PathBuf::from("leak.rs")]);
        assert_eq!(sub.effort, Effort::High);
        assert_eq!(sub.mode, ComposerMode::Build);
        assert_eq!(sub.permission, Permission::Full);
        // Box and chips cleared, pills kept.
        assert!(s.text.is_empty() && s.attachments.is_empty());
        assert_eq!((s.effort, s.mode), (Effort::High, ComposerMode::Build));
    }

    #[test]
    fn pill_defaults_match_the_web_composer() {
        let s = ComposerState::new();
        assert_eq!(s.effort, Effort::Medium);
        assert_eq!(s.mode, ComposerMode::Chat);
        assert_eq!(s.permission, Permission::Full);
        assert_eq!(
            ComposerMode::Plan.description(),
            "Plan only — no edits or commands."
        );
        assert_eq!(Permission::Supervised.as_str(), "supervised");
    }
}
