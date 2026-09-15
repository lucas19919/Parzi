//! Docs reader: project documents plus artifact versions of the thread.
//!
//! Mirrors `DocReader.svelte`: quick tabs, a sticky toolbar (title, kind,
//! version, lines, preview/raw, copy), an outline column, and the body.
//! Markdown renders as selectable text in v1 (the transcript markdown
//! renderer is a separate lane); the outline is a static list on purpose:
//! no control without a handler.

/// Largest document opened (chars). Bigger files fail with a message.
pub const MAX_DOC_CHARS: usize = 200_000;
/// Longest accepted doc path (relative to the Parzi home dir).
pub const MAX_PATH_LEN: usize = 256;

/// Preview (prose) vs raw (source) body view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DocView {
    #[default]
    Preview,
    Raw,
}

/// A quick-tab candidate (SYSTEM.md, PLAN.md, ...).
#[derive(Debug, Clone)]
pub struct DocEntry {
    pub label: String,
    /// Relative to the Parzi home dir, e.g. `projects/demo/SYSTEM.md`.
    pub path: String,
}

/// The open document.
#[derive(Debug, Clone)]
pub struct OpenDoc {
    pub title: String,
    pub path: String,
    pub content: String,
}

impl OpenDoc {
    /// Line count for the toolbar badge.
    #[must_use]
    pub fn lines(&self) -> usize {
        if self.content.is_empty() {
            0
        } else {
            self.content.lines().count()
        }
    }
}

/// One artifact version known to the thread (content itself is fetched by
/// the transcript lane; the reader shows tabs and metadata).
#[derive(Debug, Clone)]
pub struct ArtifactRef {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub language: Option<String>,
    pub version: u32,
}

/// One markdown heading for the outline.
#[derive(Debug, Clone)]
pub struct TocEntry {
    pub level: u8,
    pub text: String,
    pub idx: usize,
}

/// Reject escapes before touching the filesystem: relative paths under the
/// Parzi home only, no `..`, no absolute paths, no drive specs.
pub fn check_doc_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.len() > MAX_PATH_LEN {
        return Err(format!("bad document path (1-{MAX_PATH_LEN} chars)"));
    }
    let bad = path.starts_with('/')
        || path.starts_with('\\')
        || path.contains(':')
        || path.split(['/', '\\']).any(|seg| seg == "..");
    if bad {
        return Err(format!("document path escapes the workspace: {path}"));
    }
    Ok(())
}

/// Headings (`#` to `####`) outside fenced blocks, markup stripped.
#[must_use]
pub fn toc_of(content: &str) -> Vec<TocEntry> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for raw in content.lines() {
        let line = raw.trim_end();
        let fence = line.trim_start();
        if fence.starts_with("```") || fence.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let level = line.chars().take_while(|c| *c == '#').count();
        if !(1..=4).contains(&level) {
            continue;
        }
        let Some(rest) = line[level..].strip_prefix([' ', '\t']) else {
            continue;
        };
        let text: String = rest
            .trim_end_matches(['#', ' ', '\t'])
            .trim_end()
            .chars()
            .filter(|c| !matches!(c, '*' | '_' | '`'))
            .collect();
        if !text.trim().is_empty() {
            out.push(TocEntry {
                level: level as u8,
                text: text.trim().to_string(),
                idx: out.len(),
            });
        }
    }
    out
}

/// Newest version per artifact id, in first-seen order (version menu data).
#[must_use]
pub fn latest_per_id(all: &[ArtifactRef]) -> Vec<ArtifactRef> {
    let mut out: Vec<ArtifactRef> = Vec::new();
    for a in all {
        if let Some(cur) = out.iter_mut().find(|x| x.id == a.id) {
            if a.version > cur.version {
                *cur = a.clone();
            }
        } else {
            out.push(a.clone());
        }
    }
    out
}

/// Save-as filename: `{id}-v{version}.{ext}` for artifacts.
#[must_use]
pub fn download_name_artifact(a: &ArtifactRef) -> String {
    let ext = match a.kind.as_str() {
        "markdown" => "md",
        "svg" | "html" | "diff" | "json" | "csv" => a.kind.as_str(),
        "code" => a
            .language
            .as_deref()
            .filter(|l| !l.is_empty())
            .unwrap_or("txt"),
        _ => "txt",
    };
    format!("{}-v{}.{}", a.id, a.version, ext)
}

/// Reader state: tabs, open doc, artifact refs, view mode.
#[derive(Debug, Default)]
pub struct DocsState {
    pub docs: Vec<DocEntry>,
    pub active: Option<OpenDoc>,
    pub artifacts: Vec<ArtifactRef>,
    pub active_artifact: Option<ArtifactRef>,
    pub view: DocView,
    pub loading: bool,
    pub status: String,
}

impl DocsState {
    /// Open a workspace document (guarded read, size-capped).
    ///
    /// # Errors
    /// Path escapes, missing files, or oversize content.
    pub fn open_doc(&mut self, entry: &DocEntry) -> Result<(), String> {
        check_doc_path(&entry.path)?;
        let home = parzi_core::paths::parzi_dir().map_err(|e| format!("locate home: {e}"))?;
        let content = std::fs::read_to_string(home.join(&entry.path))
            .map_err(|e| format!("open {}: {e}", entry.path))?;
        if content.chars().count() > MAX_DOC_CHARS {
            return Err(format!("{} exceeds {MAX_DOC_CHARS} chars", entry.path));
        }
        self.active = Some(OpenDoc {
            title: entry.label.clone(),
            path: entry.path.clone(),
            content,
        });
        self.active_artifact = None;
        self.status.clear();
        Ok(())
    }

    /// Select an artifact tab (content arrives via the transcript lane).
    pub fn select_artifact(&mut self, id: &str) {
        self.active_artifact = latest_per_id(&self.artifacts)
            .into_iter()
            .find(|a| a.id == id);
        if self.active_artifact.is_some() {
            self.active = None;
        }
    }

    /// Outline of the open document.
    #[must_use]
    pub fn toc(&self) -> Vec<TocEntry> {
        self.active
            .as_ref()
            .map_or_else(Vec::new, |d| toc_of(&d.content))
    }
}

/// Thin reader: quick tabs, toolbar, outline, selectable body.
pub fn show(ui: &mut egui::Ui, s: &mut DocsState) {
    ui.horizontal_wrapped(|ui| {
        for doc in s.docs.clone() {
            let on = s.active.as_ref().is_some_and(|a| a.path == doc.path);
            if ui.selectable_label(on, &doc.label).clicked() {
                let _ = s.open_doc(&doc).map_err(|e| s.status = e);
            }
        }
        for art in latest_per_id(&s.artifacts) {
            let on = s.active_artifact.as_ref().is_some_and(|a| a.id == art.id);
            if ui.selectable_label(on, &art.title).clicked() {
                s.select_artifact(&art.id);
            }
        }
    });
    ui.separator();
    if let Some(art) = &s.active_artifact {
        ui.horizontal(|ui| {
            ui.strong(&art.title);
            ui.label(format!("{} v{}", art.kind, art.version));
            ui.label(download_name_artifact(art));
        });
        ui.label("Artifact content opens from the thread (transcript lane).");
    } else if let Some(doc) = &s.active {
        ui.horizontal(|ui| {
            ui.strong(&doc.title);
            ui.label(format!("{} lines", doc.lines()));
            ui.selectable_value(&mut s.view, DocView::Preview, "Preview");
            ui.selectable_value(&mut s.view, DocView::Raw, "Raw");
            if ui.small_button("copy").clicked() {
                ui.ctx().copy_text(doc.content.clone());
            }
        });
        ui.separator();
        let toc = toc_of(&doc.content);
        ui.horizontal(|ui| {
            if toc.len() > 2 {
                ui.vertical(|ui| {
                    ui.weak("Outline");
                    for h in toc {
                        ui.label(indent_toc(h.level, &h.text));
                    }
                });
                ui.separator();
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                let text = egui::RichText::new(&doc.content);
                let text = match s.view {
                    DocView::Raw => text.monospace().small(),
                    DocView::Preview => text,
                };
                ui.add(egui::Label::new(text).selectable(true).wrap());
            });
        });
    } else if s.loading {
        ui.label("Loading...");
    } else {
        ui.label("Nothing open: pick a project document above.");
    }
    if !s.status.is_empty() {
        ui.label(s.status.as_str());
    }
}

/// Outline row with depth indentation (static text, not a control: jumping
/// needs rendered heading anchors from the transcript lane).
fn indent_toc(level: u8, text: &str) -> String {
    format!(
        "{:>w$} {text}",
        "",
        w = (level as usize).saturating_sub(1) * 2
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_gate() {
        assert!(check_doc_path("projects/demo/SYSTEM.md").is_ok());
        for bad in ["", "../escape.md", "/abs.md", "C:/win.md", "a/../../b.md"] {
            assert!(check_doc_path(bad).is_err(), "{bad}");
        }
        assert!(check_doc_path(&"a".repeat(MAX_PATH_LEN + 1)).is_err());
    }

    #[test]
    fn toc_skips_fences_and_strips_markup() {
        let md = "# Title\n\n```\n# not a heading\n```\n\n## Sec *one*\n### `code` _it_\n";
        let toc = toc_of(md);
        assert_eq!(toc.len(), 3);
        assert_eq!((toc[0].level, toc[0].text.as_str()), (1, "Title"));
        assert_eq!(toc[1].text, "Sec one");
        assert_eq!((toc[2].level, toc[2].text.as_str()), (3, "code it"));
        assert!(toc_of("no headings\njust text\n").is_empty());
    }

    #[test]
    fn latest_versions_and_download_names() {
        let mk = |version| ArtifactRef {
            id: "a".into(),
            title: "A".into(),
            kind: "code".into(),
            language: Some("rust".into()),
            version,
        };
        let md = ArtifactRef {
            id: "b".into(),
            title: "B".into(),
            kind: "markdown".into(),
            language: None,
            version: 1,
        };
        let latest = latest_per_id(&[mk(1), mk(2), md]);
        assert_eq!(latest.len(), 2);
        assert_eq!(latest[0].version, 2);
        assert_eq!(download_name_artifact(&latest[0]), "a-v2.rust");
        assert_eq!(download_name_artifact(&latest[1]), "b-v1.md");
    }

    #[test]
    fn open_doc_rejects_bad_path_without_fs() {
        let mut s = DocsState::default();
        let entry = DocEntry {
            label: "x".into(),
            path: "../escape.md".into(),
        };
        assert!(s.open_doc(&entry).is_err());
        assert!(s.active.is_none());
        s.select_artifact("missing");
        assert!(s.active_artifact.is_none());
    }

    #[test]
    fn open_doc_counts_lines_and_indents_outline() {
        let doc = OpenDoc {
            title: "t".into(),
            path: "p".into(),
            content: "a\nb\nc".into(),
        };
        assert_eq!(doc.lines(), 3);
        let empty = OpenDoc {
            title: "t".into(),
            path: "p".into(),
            content: String::new(),
        };
        assert_eq!(empty.lines(), 0);
        assert_eq!(indent_toc(1, "Title"), " Title");
        assert_eq!(indent_toc(3, "Deep"), "     Deep");
    }
}
