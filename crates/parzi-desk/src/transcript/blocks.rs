//! Transcript content blocks: the AST between markdown events and paint.
//!
//! Blocks own plain data (`LayoutJob`s, strings, colours) so lowering and
//! highlighting stay pure: no `Context`, no GPU, unit-testable. Paint lives
//! in [`super::paint_block`], which the tests never call.

use std::sync::OnceLock;

use egui::{Color32, FontFamily, FontId};
use epaint::text::{LayoutJob, TextFormat, VariationCoords};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SynColor, FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;

use crate::theme::Tokens;

/// Lines of code shown before the expander (PLAN §4: same rule as today).
pub const MAX_VISIBLE_CODE_LINES: usize = 30;
/// Hanging indent per nested list level in points (PLAN §4).
pub const LIST_INDENT_PX: f32 = 20.0;
/// Spaces per nesting level baked into list-item job text (paint maps depth
/// to [`LIST_INDENT_PX`]).
pub const LIST_INDENT_SPACES: usize = 2;

/// One renderable unit of the transcript (PLAN §4: each becomes one
/// `LayoutJob` or one custom block).
#[derive(Clone, Debug)]
pub enum ContentBlock {
    /// Paragraph, heading, or list item. Items carry their marker plus
    /// [`LIST_INDENT_SPACES`] spaces per nesting level as a text prefix.
    Prose(LayoutJob),
    /// Fenced/indented code with highlighting and copy data.
    CodeFence(CodeFence),
    /// Whole quote as one job; the rule and inset are paint concerns.
    Blockquote(Blockquote),
    /// Grid data; cell wrapping is a paint concern.
    Table(TableBlock),
    /// Tool-call / approval summary card (filled by the live-run bridge).
    ToolCard(ToolCard),
    /// `---`.
    Divider,
}

/// Fenced code: highlighted spans plus the raw text for the copy button.
/// `spans` concatenated always equal `code`.
#[derive(Clone, Debug)]
pub struct CodeFence {
    /// Info-string language tag (`""` when unspecified).
    pub language: String,
    /// Raw code; the copy affordance copies exactly this.
    pub code: String,
    pub spans: Vec<CodeSpan>,
    /// True past [`MAX_VISIBLE_CODE_LINES`]; paint shows head + expander.
    pub clamped: bool,
}

/// One highlighted run inside a [`CodeFence`].
#[derive(Clone, Debug, PartialEq)]
pub struct CodeSpan {
    pub text: String,
    pub color: Color32,
    pub bold: bool,
}

impl CodeSpan {
    fn new(text: String, color: Color32, bold: bool) -> Self {
        Self { text, color, bold }
    }
}

/// A blockquote's flattened text.
#[derive(Clone, Debug)]
pub struct Blockquote {
    pub job: LayoutJob,
}

/// Table data with plain-text cells (inline styles are flattened).
#[derive(Clone, Debug, Default)]
pub struct TableBlock {
    pub alignments: Vec<TableAlign>,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Column alignment from the delimiter row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Tool-call summary card state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolStatus {
    #[default]
    Pending,
    Running,
    Done,
    Denied,
}

/// Tool-call summary card (PLAN §2 approver queue renders through this).
#[derive(Clone, Debug)]
pub struct ToolCard {
    pub name: String,
    pub status: ToolStatus,
    pub summary: String,
}

impl CodeFence {
    /// `spans` concatenated must equal `code`.
    pub fn new(language: String, code: String, spans: Vec<CodeSpan>) -> Self {
        let clamped = code.lines().count() > MAX_VISIBLE_CODE_LINES;
        Self {
            language,
            code,
            spans,
            clamped,
        }
    }

    pub fn line_count(&self) -> usize {
        self.code.lines().count()
    }

    pub fn is_clamped(&self) -> bool {
        self.clamped
    }

    /// Badge text for the fence header. Empty language shows as plain.
    pub fn language_or_plain(&self) -> &str {
        if self.language.is_empty() {
            "plain"
        } else {
            &self.language
        }
    }

    /// Exact text the copy button writes to the clipboard.
    pub fn copy_text(&self) -> &str {
        &self.code
    }

    /// Head shown while clamped: the first [`MAX_VISIBLE_CODE_LINES`] lines.
    pub fn visible_text(&self) -> String {
        self.head_lines(MAX_VISIBLE_CODE_LINES)
    }

    /// Hidden tail line count (0 unless clamped).
    pub fn hidden_lines(&self) -> usize {
        self.line_count().saturating_sub(MAX_VISIBLE_CODE_LINES)
    }

    fn head_lines(&self, max: usize) -> String {
        let mut taken = self.code.split('\n').take(max);
        let mut out = String::from(taken.next().unwrap_or(""));
        for line in taken {
            out.push('\n');
            out.push_str(line);
        }
        out
    }
}

impl TableBlock {
    /// Widest row: headers, alignments, or any body row.
    pub fn columns(&self) -> usize {
        let widest = self.rows.iter().map(Vec::len).max().unwrap_or(0);
        widest.max(self.headers.len()).max(self.alignments.len())
    }

    pub fn is_empty(&self) -> bool {
        self.headers.is_empty() && self.rows.is_empty()
    }
}

impl ContentBlock {
    /// Borrow the shaped text of prose blocks.
    pub fn prose_text(&self) -> Option<&str> {
        match self {
            Self::Prose(job) => Some(job.text.as_str()),
            _ => None,
        }
    }

    pub fn is_divider(&self) -> bool {
        matches!(self, Self::Divider)
    }
}

// --- Code jobs: spans themed in markdown.rs become LayoutJobs here ---

/// Full-code job for a fence (mono, span colours).
pub fn code_job(fence: &CodeFence, tokens: &Tokens) -> LayoutJob {
    code_job_limited(fence, tokens, usize::MAX)
}

/// Clamped head job: the first [`MAX_VISIBLE_CODE_LINES`] lines.
pub fn code_head_job(fence: &CodeFence, tokens: &Tokens) -> LayoutJob {
    code_job_limited(fence, tokens, MAX_VISIBLE_CODE_LINES)
}

fn code_job_limited(fence: &CodeFence, tokens: &Tokens, max_lines: usize) -> LayoutJob {
    let base = mono_fmt(tokens, tokens.text);
    let mut job = LayoutJob::single_section(String::new(), base.clone());
    let mut line = 1usize;
    'spans: for span in &fence.spans {
        let mut fmt = mono_fmt(tokens, span.color);
        if span.bold {
            fmt.coords = VariationCoords::new([(b"wght", 700.0)]);
        }
        for (piece_no, piece) in span.text.split('\n').enumerate() {
            if piece_no > 0 {
                line += 1;
                if line > max_lines {
                    break 'spans;
                }
                job.append("\n", 0.0, base.clone());
            }
            if !piece.is_empty() {
                job.append(piece, 0.0, fmt.clone());
            }
        }
    }
    job
}

fn mono_fmt(tokens: &Tokens, color: Color32) -> TextFormat {
    TextFormat {
        font_id: FontId::new(tokens.mono_size, FontFamily::Monospace),
        color,
        ..Default::default()
    }
}

// --- Syntect highlighting, themed from the skeleton tokens ---

static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
static THEMES: OnceLock<ThemeSet> = OnceLock::new();

/// Highlight `code`, falling back to one plain span. Lossless: the spans
/// always concatenate back to `code` (anything else degrades to plain).
pub(crate) fn highlight_code(code: &str, language: &str, tokens: &Tokens) -> Vec<CodeSpan> {
    let plain = || vec![CodeSpan::new(code.to_owned(), tokens.text, false)];
    if code.is_empty() {
        return Vec::new();
    }
    let syntaxes = SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines);
    let syntax = if language.is_empty() {
        syntaxes.find_syntax_plain_text()
    } else {
        syntaxes
            .find_syntax_by_extension(language)
            .or_else(|| syntaxes.find_syntax_by_token(language))
            .unwrap_or_else(|| syntaxes.find_syntax_plain_text())
    };
    let themes = THEMES.get_or_init(ThemeSet::load_defaults);
    let Some(base) = themes
        .themes
        .get("base16-ocean.dark")
        .or_else(|| themes.themes.values().next())
    else {
        return plain();
    };
    // Token colours from syntect, base colours from our theme.
    let mut theme = base.clone();
    theme.settings.foreground = Some(syn_color(tokens.text));
    theme.settings.background = Some(syn_color(tokens.bar));
    let mut highlighter = HighlightLines::new(syntax, &theme);
    let mut spans = Vec::new();
    for line in code.split_inclusive('\n') {
        match highlighter.highlight_line(line, syntaxes) {
            Ok(runs) => {
                for (style, part) in runs {
                    if part.is_empty() {
                        continue;
                    }
                    let color = egui_color(style.foreground);
                    let bold = style.font_style.contains(FontStyle::BOLD);
                    spans.push(CodeSpan::new(part.to_owned(), color, bold));
                }
            }
            Err(_) => spans.push(CodeSpan::new(line.to_owned(), tokens.text, false)),
        }
    }
    if spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>()
        != code
    {
        return plain();
    }
    spans
}

fn syn_color(color: Color32) -> SynColor {
    SynColor {
        r: color.r(),
        g: color.g(),
        b: color.b(),
        a: color.a(),
    }
}

fn egui_color(color: SynColor) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fence(lines: usize) -> CodeFence {
        let mut code = String::new();
        for i in 0..lines {
            if i > 0 {
                code.push('\n');
            }
            code.push_str(&format!("line {i}"));
        }
        let spans = vec![CodeSpan::new(code.clone(), Color32::WHITE, false)];
        CodeFence::new("rust".to_owned(), code, spans)
    }

    #[test]
    fn transcript_clamp_marks_long_fences() {
        assert!(!fence(MAX_VISIBLE_CODE_LINES).is_clamped());
        let long = fence(MAX_VISIBLE_CODE_LINES + 1);
        assert!(long.is_clamped());
        assert_eq!(long.hidden_lines(), 1);
        assert_eq!(long.visible_text().lines().count(), MAX_VISIBLE_CODE_LINES);
        assert_eq!(long.copy_text(), long.code);
    }

    #[test]
    fn transcript_fence_badge_and_counts() {
        let f = fence(3);
        assert_eq!(f.line_count(), 3);
        assert_eq!(f.language_or_plain(), "rust");
        let plain = CodeFence::new(String::new(), String::new(), vec![]);
        assert_eq!(plain.language_or_plain(), "plain");
        assert_eq!(plain.line_count(), 0);
        assert!(!plain.is_clamped());
    }

    #[test]
    fn transcript_table_columns_and_empty() {
        let table = TableBlock {
            alignments: vec![TableAlign::Left, TableAlign::Right],
            headers: vec!["a".to_owned()],
            rows: vec![vec!["1".to_owned(), "2".to_owned(), "3".to_owned()]],
        };
        assert_eq!(table.columns(), 3);
        assert!(!table.is_empty());
        assert!(TableBlock::default().is_empty());
    }

    #[test]
    fn transcript_prose_helpers() {
        let job = LayoutJob::single_section("hi".to_owned(), Default::default());
        let block = ContentBlock::Prose(job);
        assert_eq!(block.prose_text(), Some("hi"));
        assert!(!block.is_divider());
        assert!(ContentBlock::Divider.is_divider());
        assert_eq!(ContentBlock::Divider.prose_text(), None);
    }

    #[test]
    fn transcript_code_jobs_rebuild_losslessly() {
        let tokens = Tokens::from_theme(&parzi_core::theme::Theme::default());
        let full = fence(3);
        assert_eq!(code_job(&full, &tokens).text, full.code);
        let long = fence(MAX_VISIBLE_CODE_LINES + 5);
        let head = code_head_job(&long, &tokens).text;
        assert_eq!(head.lines().count(), MAX_VISIBLE_CODE_LINES);
        let all = code_job(&long, &tokens).text;
        assert_eq!(all.lines().count(), MAX_VISIBLE_CODE_LINES + 5);
    }

    #[test]
    fn transcript_highlighting_is_lossless() {
        let tokens = Tokens::from_theme(&parzi_core::theme::Theme::default());
        let code = "fn main() {\n    println!(\"hi\");\n}\n";
        let spans = highlight_code(code, "rust", &tokens);
        assert_eq!(
            spans.iter().map(|s| s.text.as_str()).collect::<String>(),
            code
        );
        assert!(spans.len() > 1, "rust should produce several runs");
        assert!(highlight_code("", "rust", &tokens).is_empty());
    }
}
