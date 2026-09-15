//! `pulldown-cmark` events → [`ContentBlock`]s: pure (`&str` in, blocks out,
//! no `Context`). Raw HTML becomes literal text; an unclosed fence stays code
//! (pulldown auto-closes it at end of input). Construction helpers live in
//! [`super::blocks`] and [`super::TableBuilder`].

use super::blocks::{highlight_code, Blockquote, CodeFence, ContentBlock};
use super::{TableBuilder, LIST_INDENT_SPACES};
use crate::theme::Tokens;
use egui::{FontFamily, FontId, Stroke};
use epaint::text::{ByteIndex, LayoutJob, LayoutSection, TextFormat, VariationCoords};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// Lower one message. Re-run on every stream delta; only the tail changes.
pub fn render_markdown(text: &str, tokens: &Tokens) -> Vec<ContentBlock> {
    let mut options = Options::empty();
    options
        .insert(Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH);
    let mut lowering = Lowering::new(tokens);
    for event in Parser::new_ext(text, options) {
        lowering.event(event);
    }
    lowering.finish()
}

struct ItemCtx {
    ordered_num: Option<u64>,
    task: Option<bool>,
    body: LayoutJob,
    flushed: bool,
}

struct Lowering<'t> {
    tokens: &'t Tokens,
    body: TextFormat,
    quote_fmt: TextFormat,
    blocks: Vec<ContentBlock>,
    cur: LayoutJob,
    cur_active: bool,
    fmt: TextFormat,
    saved: Vec<TextFormat>,
    lists: Vec<Option<u64>>,
    items: Vec<ItemCtx>,
    quote: Option<LayoutJob>,
    quote_depth: u32,
    code: Option<(String, String)>,
    table: Option<TableBuilder>,
}

impl<'t> Lowering<'t> {
    fn new(tokens: &'t Tokens) -> Self {
        let body = TextFormat {
            font_id: FontId::new(tokens.body_size, FontFamily::Proportional),
            color: tokens.text,
            line_height: Some(tokens.body_size + 8.0),
            ..Default::default()
        };
        let mut quote_fmt = body.clone();
        quote_fmt.color = tokens.text_dim;
        let cur = LayoutJob::single_section(String::new(), body.clone());
        Self {
            tokens,
            body: body.clone(),
            quote_fmt,
            cur,
            fmt: body,
            blocks: Vec::new(),
            cur_active: false,
            saved: Vec::new(),
            lists: Vec::new(),
            items: Vec::new(),
            quote: None,
            quote_depth: 0,
            code: None,
            table: None,
        }
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text.into_string(), false),
            Event::Code(text) => self.text(&text.into_string(), true),
            Event::SoftBreak | Event::HardBreak => self.text("\n", false),
            Event::Rule => {
                self.flush_item_body();
                self.blocks.push(ContentBlock::Divider);
            }
            Event::TaskListMarker(checked) => {
                if let Some(item) = self.items.last_mut() {
                    item.task = Some(checked);
                }
            }
            Event::FootnoteReference(label) => {
                self.text(&format!("[^{}]", label.into_string()), false)
            }
            Event::Html(html) | Event::InlineHtml(html) => self.text(&html.into_string(), false),
            Event::InlineMath(m) | Event::DisplayMath(m) => self.text(&m.into_string(), false),
        }
    }

    /// Inline text; `mono` = code span. An open fence captures everything.
    fn text(&mut self, text: &str, mono: bool) {
        if let Some((_, buf)) = self.code.as_mut() {
            buf.push_str(text);
            return;
        }
        if text.is_empty() {
            return;
        }
        let mut fmt = self.fmt.clone();
        if mono {
            fmt.font_id = FontId::new(self.tokens.mono_size, FontFamily::Monospace);
            fmt.background = self.tokens.bar;
            fmt.expand_bg = 2.0;
        }
        let job = if self.cur_active {
            &mut self.cur
        } else if let Some(item) = self.items.last_mut() {
            &mut item.body
        } else {
            &mut self.cur
        };
        job.append(text, 0.0, fmt);
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                self.cur = self.blank();
                self.cur_active = true;
                self.fmt = if self.quote.is_some() {
                    self.quote_fmt.clone()
                } else {
                    self.body.clone()
                };
            }
            Tag::Heading { level, .. } => {
                self.cur = self.blank();
                self.cur_active = true;
                self.fmt = self.heading_fmt(level as u8);
            }
            Tag::BlockQuote(_) => {
                self.quote_depth += 1;
                if self.quote.is_none() {
                    let fmt = self.quote_fmt.clone();
                    self.quote = Some(LayoutJob::single_section(String::new(), fmt));
                }
            }
            Tag::CodeBlock(kind) => {
                let mut language = match kind {
                    CodeBlockKind::Fenced(info) => info.into_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                language.truncate(language.find(char::is_whitespace).unwrap_or(language.len()));
                self.code = Some((language, String::new()));
            }
            Tag::List(start) => {
                self.flush_item_body();
                self.lists.push(start);
            }
            Tag::Item => {
                let num = self.lists.last().copied().flatten();
                if let Some(Some(next)) = self.lists.last_mut() {
                    *next += 1;
                }
                let body = self.blank();
                self.items.push(ItemCtx {
                    ordered_num: num,
                    task: None,
                    body,
                    flushed: false,
                });
            }
            Tag::Table(aligns) => self.table = Some(TableBuilder::new(aligns)),
            Tag::TableRow => self.table_do(TableBuilder::begin_row),
            Tag::TableCell => {
                self.cur = self.blank();
                self.cur_active = true;
                self.fmt = self.body.clone();
                self.table_do(|t| t.cell_open = true);
            }
            Tag::Emphasis => self.push_fmt(|f| f.italics = true),
            Tag::Strong => self.push_fmt(|f| f.coords = VariationCoords::new([(b"wght", 600.0)])),
            Tag::Strikethrough => {
                let color = self.tokens.text;
                self.push_fmt(|f| f.strikethrough = Stroke::new(1.0, color));
            }
            Tag::Link { .. } | Tag::Image { .. } => {
                let accent = self.tokens.accent;
                self.push_fmt(|f| {
                    f.color = accent;
                    f.underline = Stroke::new(1.0, accent);
                });
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) => {
                let blank = self.blank();
                let job = std::mem::replace(&mut self.cur, blank);
                self.cur_active = false;
                self.fmt = self.body.clone();
                self.emit_prose(job);
            }
            TagEnd::BlockQuote(_) => {
                self.quote_depth = self.quote_depth.saturating_sub(1);
                if self.quote_depth == 0 {
                    if let Some(job) = self.quote.take().filter(|job| !job.text.is_empty()) {
                        self.blocks
                            .push(ContentBlock::Blockquote(Blockquote { job }));
                    }
                }
                self.fmt = self.body.clone();
            }
            TagEnd::CodeBlock => {
                if let Some((language, buf)) = self.code.take() {
                    let spans = highlight_code(&buf, &language, self.tokens);
                    self.blocks.push(ContentBlock::CodeFence(CodeFence::new(
                        language, buf, spans,
                    )));
                }
            }
            TagEnd::List(_) => _ = self.lists.pop(),
            TagEnd::Item => {
                if let Some(ctx) = self.items.pop() {
                    if !ctx.body.text.is_empty() {
                        let prefix = self.item_prefix(&ctx);
                        self.emit_prose(prefix_job(&prefix, ctx.body, &self.body));
                    }
                }
            }
            TagEnd::TableHead | TagEnd::TableRow => self.table_do(TableBuilder::end_row),
            TagEnd::TableCell => {
                let text = std::mem::take(&mut self.cur.text);
                self.cur.sections.clear();
                self.cur_active = false;
                self.fmt = self.body.clone();
                self.table_do(|t| t.push_cell(&text));
            }
            TagEnd::Table => {
                if let Some(block) = self.table.take().and_then(TableBuilder::finish) {
                    self.blocks.push(ContentBlock::Table(block));
                }
            }
            TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Link
            | TagEnd::Image => {
                self.fmt = self.saved.pop().unwrap_or_else(|| self.body.clone());
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Vec<ContentBlock> {
        if !self.cur.text.is_empty()
            && self.items.is_empty()
            && self.quote.is_none()
            && !self.table.as_ref().is_some_and(|t| t.cell_open)
        {
            let blank = self.blank();
            let job = std::mem::replace(&mut self.cur, blank);
            self.blocks.push(ContentBlock::Prose(job));
        }
        self.blocks
    }

    fn blank(&self) -> LayoutJob {
        LayoutJob::single_section(String::new(), self.body.clone())
    }

    fn push_fmt(&mut self, patch: impl FnOnce(&mut TextFormat)) {
        self.saved.push(self.fmt.clone());
        patch(&mut self.fmt);
    }

    fn table_do(&mut self, patch: impl FnOnce(&mut TableBuilder)) {
        if let Some(table) = self.table.as_mut() {
            patch(table);
        }
    }
    fn heading_fmt(&self, level: u8) -> TextFormat {
        let size = self.tokens.body_size
            + match level {
                1 => 6.0,
                2 => 3.0,
                _ => 1.0,
            };
        TextFormat {
            font_id: FontId::new(size, FontFamily::Proportional),
            color: self.tokens.text,
            line_height: Some(size + 7.0),
            coords: VariationCoords::new([(b"wght", 600.0)]),
            ..Default::default()
        }
    }

    /// Flush a pending item body before a nested list/rule so child blocks land after it.
    fn flush_item_body(&mut self) {
        let Some(mut ctx) = self.items.pop() else {
            return;
        };
        if !ctx.body.text.is_empty() {
            let prefix = self.item_prefix(&ctx);
            let blank = self.blank();
            let job = prefix_job(&prefix, std::mem::replace(&mut ctx.body, blank), &self.body);
            ctx.flushed = true;
            self.emit_prose(job);
        }
        self.items.push(ctx);
    }

    fn item_prefix(&self, ctx: &ItemCtx) -> String {
        let pad = " ".repeat(self.lists.len().saturating_sub(1) * LIST_INDENT_SPACES);
        if ctx.flushed {
            return pad;
        }
        let marker = match (ctx.ordered_num, ctx.task) {
            (Some(n), Some(true)) => format!("{n}. [x] "),
            (Some(n), Some(false)) => format!("{n}. [ ] "),
            (Some(n), None) => format!("{n}. "),
            (None, Some(true)) => "[x] ".to_owned(),
            (None, Some(false)) => "[ ] ".to_owned(),
            (None, None) => "• ".to_owned(),
        };
        format!("{pad}{marker}")
    }

    /// Route a finished prose job to the innermost open container.
    fn emit_prose(&mut self, job: LayoutJob) {
        if job.text.is_empty() {
            return;
        }
        if self.table.as_ref().is_some_and(|t| t.cell_open) {
            self.table_do(|t| t.accumulate(&job.text));
        } else if let Some(item) = self.items.last_mut() {
            merge(&mut item.body, job);
        } else if let Some(quote) = self.quote.as_mut() {
            merge(quote, job);
        } else {
            self.blocks.push(ContentBlock::Prose(job));
        }
    }
}

/// Append `src` after `dst`, shifting section byte ranges so formats survive.
fn merge(dst: &mut LayoutJob, src: LayoutJob) {
    if src.text.is_empty() {
        return;
    }
    if !dst.text.is_empty() {
        dst.text.push_str("\n\n");
    }
    let offset = dst.text.len();
    dst.text.push_str(&src.text);
    dst.sections
        .extend(src.sections.into_iter().map(|mut section| {
            section.byte_range.start += offset;
            section.byte_range.end += offset;
            section
        }));
}

/// Prepend a bullet/number marker as its own section.
fn prefix_job(prefix: &str, mut job: LayoutJob, fmt: &TextFormat) -> LayoutJob {
    if job.text.is_empty() {
        return LayoutJob::single_section(prefix.to_owned(), fmt.clone());
    }
    let len = prefix.len();
    for section in &mut job.sections {
        section.byte_range.start += len;
        section.byte_range.end += len;
    }
    job.text.insert_str(0, prefix);
    let range = ByteIndex::ZERO..ByteIndex::from(len);
    job.sections.insert(
        0,
        LayoutSection {
            leading_space: 0.0,
            byte_range: range,
            format: fmt.clone(),
        },
    );
    job
}
