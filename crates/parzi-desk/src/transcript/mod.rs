//! Transcript reading surface: markdown → blocks → scrollback (PLAN §4, M1).
//!
//! Pipeline: [`markdown::render_markdown`] lowers one message to
//! [`blocks::ContentBlock`]s (pure, no `Context`); [`cache::ScrollbackCache`]
//! keeps per-block heights plus prefix sums so the viewport maps to a block
//! range with two binary searches. [`blocks::code_job`] turns a fence back
//! into a shaped job at paint time. Shared construction helpers
//! ([`TableBuilder`], [`blocks::highlight_code`]) and the paint entry
//! ([`paint_block`]) live here. Paint is never called by the unit tests.

pub mod blocks;
pub mod cache;
pub mod markdown;

use crate::theme::Tokens;
use pulldown_cmark::Alignment;

pub use blocks::{
    code_head_job, code_job, Blockquote, CodeFence, CodeSpan, ContentBlock, TableAlign, TableBlock,
    ToolCard, ToolStatus, LIST_INDENT_PX, LIST_INDENT_SPACES, MAX_VISIBLE_CODE_LINES,
};
pub use cache::ScrollbackCache;
pub use markdown::render_markdown;

/// Open-table accumulator feeding [`TableBlock`]. The lowering calls
/// `begin_row` / `push_cell` / `end_row` per event; [`TableBuilder::finish`]
/// takes the first finished row as the header row (pulldown always emits
/// the head first, so no `in_head` flag is needed).
#[derive(Clone, Debug, Default)]
pub(crate) struct TableBuilder {
    aligns: Vec<blocks::TableAlign>,
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    buf: String,
    pub(crate) cell_open: bool,
}

impl TableBuilder {
    pub(crate) fn new(aligns: Vec<Alignment>) -> Self {
        let aligns = aligns.into_iter().map(map_align).collect();
        Self {
            aligns,
            rows: Vec::new(),
            row: Vec::new(),
            buf: String::new(),
            cell_open: false,
        }
    }

    pub(crate) fn begin_row(&mut self) {
        self.row.clear();
    }

    /// Accumulate paragraph text into the open cell.
    pub(crate) fn accumulate(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if !self.buf.is_empty() {
            self.buf.push('\n');
        }
        self.buf.push_str(text);
    }

    /// Close the open cell, moving its text into the row.
    pub(crate) fn push_cell(&mut self, text: &str) {
        self.accumulate(text);
        self.row.push(std::mem::take(&mut self.buf));
        self.cell_open = false;
    }

    pub(crate) fn end_row(&mut self) {
        self.rows.push(std::mem::take(&mut self.row));
    }

    /// First row = headers; `None` when the table holds no rows.
    pub(crate) fn finish(mut self) -> Option<blocks::TableBlock> {
        if self.rows.is_empty() {
            return None;
        }
        let headers = self.rows.remove(0);
        Some(blocks::TableBlock {
            alignments: self.aligns,
            headers,
            rows: self.rows,
        })
    }
}

fn map_align(align: Alignment) -> blocks::TableAlign {
    match align {
        Alignment::Left | Alignment::None => blocks::TableAlign::Left,
        Alignment::Center => blocks::TableAlign::Center,
        Alignment::Right => blocks::TableAlign::Right,
    }
}

// --- Tiny paint fns (never called by tests; measuring stays in cache.rs) ---

/// Paint one block. Headings/list depth are already baked into the jobs;
/// heights are measured by the caller into [`ScrollbackCache`].
pub fn paint_block(ui: &mut egui::Ui, block: &ContentBlock, tokens: &Tokens) {
    match block {
        ContentBlock::Prose(job) => {
            ui.add(egui::Label::new(job.clone()).selectable(true));
        }
        ContentBlock::CodeFence(fence) => paint_code(ui, fence, tokens),
        ContentBlock::Blockquote(quote) => {
            let response = ui.add(egui::Label::new(quote.job.clone()).selectable(true));
            let rect = response.rect;
            ui.painter().vline(
                rect.left() - 6.0,
                rect.y_range(),
                egui::Stroke::new(2.0, tokens.border),
            );
        }
        ContentBlock::Table(table) => paint_table(ui, table),
        ContentBlock::ToolCard(card) => {
            ui.group(|ui| {
                ui.label(format!("[{}] {}", status_glyph(card.status), card.name));
                ui.label(&card.summary);
            });
        }
        ContentBlock::Divider => {
            ui.add(egui::Separator::default());
        }
    }
}

fn paint_code(ui: &mut egui::Ui, fence: &CodeFence, tokens: &Tokens) {
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(fence.language_or_plain())
                    .monospace()
                    .weak(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("copy").clicked() {
                    ui.ctx().copy_text(fence.copy_text().to_owned());
                }
            });
        });
        if fence.is_clamped() {
            let head = code_head_job(fence, tokens);
            ui.add(egui::Label::new(head).selectable(true));
            ui.collapsing(format!("show {} more lines", fence.hidden_lines()), |ui| {
                let full = code_job(fence, tokens);
                ui.add(egui::Label::new(full).selectable(true));
            });
        } else {
            ui.add(egui::Label::new(code_job(fence, tokens)).selectable(true));
        }
    });
}

fn paint_table(ui: &mut egui::Ui, table: &blocks::TableBlock) {
    egui::Grid::new(("transcript-table", table.headers.len(), table.rows.len()))
        .striped(true)
        .show(ui, |ui| {
            for cell in &table.headers {
                ui.label(egui::RichText::new(cell).strong());
            }
            if !table.headers.is_empty() {
                ui.end_row();
            }
            for row in &table.rows {
                for cell in row {
                    ui.label(cell);
                }
                ui.end_row();
            }
        });
}

fn status_glyph(status: ToolStatus) -> &'static str {
    match status {
        ToolStatus::Pending => "...",
        ToolStatus::Running => ">>",
        ToolStatus::Done => "ok",
        ToolStatus::Denied => "xx",
    }
}

#[cfg(test)]
mod tests {
    use egui::{FontFamily, Stroke};

    use crate::theme::Tokens;
    use pulldown_cmark::Alignment;

    use super::blocks::ContentBlock;
    use super::markdown::render_markdown;

    fn tokens() -> Tokens {
        Tokens::from_theme(&parzi_core::theme::Theme::default())
    }

    fn render(markdown: &str) -> Vec<ContentBlock> {
        render_markdown(markdown, &tokens())
    }

    /// One-line shape per block for table-driven asserts.
    fn shape(block: &ContentBlock) -> String {
        match block {
            ContentBlock::Prose(job) => job.text.clone(),
            ContentBlock::CodeFence(fence) => {
                format!("fence:{}:{}", fence.language, fence.line_count())
            }
            ContentBlock::Blockquote(quote) => format!("quote:{}", quote.job.text),
            ContentBlock::Table(table) => {
                let mut out = format!("table:{}", table.headers.join(","));
                for row in &table.rows {
                    out.push('|');
                    out.push_str(&row.join(","));
                }
                out
            }
            ContentBlock::ToolCard(card) => format!("tool:{}", card.name),
            ContentBlock::Divider => "hr".to_owned(),
        }
    }

    /// `eq(md, &["block", ...])`: one line per markdown construct.
    fn eq(markdown: &str, want: &[&str]) {
        let got: Vec<String> = render(markdown).iter().map(shape).collect();
        let want: Vec<String> = want.iter().map(ToString::to_string).collect();
        assert_eq!(got, want, "input: {markdown:?}");
    }

    /// Non-empty section formats (fresh jobs start with one empty section).
    fn runs(job: &epaint::text::LayoutJob) -> Vec<(&str, epaint::text::TextFormat)> {
        job.sections
            .iter()
            .filter(|section| !section.byte_range.is_empty())
            .map(|section| {
                let start = usize::from(section.byte_range.start);
                let end = usize::from(section.byte_range.end);
                (&job.text[start..end], section.format.clone())
            })
            .collect()
    }

    #[test]
    fn transcript_block_constructs() {
        eq("# Hi", &["Hi"]);
        eq("para **bold** text", &["para bold text"]);
        eq("- a\n- b", &["• a", "• b"]);
        eq("1. a\n2. b", &["1. a", "2. b"]);
        eq("- a\n  - b", &["• a", "  • b"]);
        eq("- [ ] a\n- [x] b", &["[ ] a", "[x] b"]);
        eq("> a\n>\n> b", &["quote:a\n\nb"]);
        eq("```rust\nlet x = 1;\n```", &["fence:rust:1"]);
        eq("```\ncode\n```", &["fence::1"]);
        eq("| a | b |\n| --- | ---: |\n| 1 | 2 |", &["table:a,b|1,2"]);
        eq("x\n\n---\n\ny", &["x", "hr", "y"]);
        assert!(render("").is_empty());
        assert!(render("\n\n").is_empty());
    }

    #[test]
    fn transcript_inline_styles() {
        let tk = tokens();
        let blocks = render("a **bold** and *ital* and `code` and [link](https://x.y) and ~gone~");
        let ContentBlock::Prose(job) = &blocks[0] else {
            panic!("prose, got {:?}", blocks[0])
        };
        assert!(
            !job.text.contains("https://x.y"),
            "URLs stay out of the text"
        );
        let run = |needle: &str| {
            runs(job)
                .into_iter()
                .find(|(text, _)| *text == needle)
                .map(|(_, fmt)| fmt)
        };
        assert!(format!("{:?}", run("bold").expect("bold").coords).contains("600"));
        assert!(run("ital").expect("italic").italics);
        let code = run("code").expect("code");
        assert_eq!(code.font_id.family, FontFamily::Monospace);
        assert_eq!(code.background, tk.bar);
        let link = run("link").expect("link");
        assert_eq!(link.color, tk.accent);
        assert_ne!(link.underline, Stroke::NONE);
        assert_ne!(run("gone").expect("strike").strikethrough, Stroke::NONE);
    }

    #[test]
    fn transcript_fences_clamp_and_stream() {
        let long = (1..=35)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let blocks = render(&format!("```\n{long}\n```"));
        let ContentBlock::CodeFence(fence) = &blocks[0] else {
            panic!("fence, got {:?}", blocks[0])
        };
        assert!(fence.is_clamped());
        assert_eq!(fence.hidden_lines(), 5);
        // Unclosed fence while streaming: the tail stays code, never prose.
        let blocks = render("```rust\nlet x = 1;\nmore code, no closing fence");
        assert_eq!(blocks.len(), 1, "tail must not flip to prose: {blocks:?}");
        assert!(matches!(&blocks[0], ContentBlock::CodeFence(_)));
    }

    #[test]
    fn transcript_headings_shrink_by_level() {
        let blocks = render("# One\n\n### Three");
        let sizes: Vec<f32> = blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Prose(job) => runs(job).first().map(|(_, fmt)| fmt.font_id.size),
                _ => None,
            })
            .collect();
        assert_eq!(sizes.len(), 2);
        assert!(
            sizes[0] > sizes[1],
            "h1 must read bigger than h3: {sizes:?}"
        );
    }
}
