//! Scrollback cache: per-block heights plus prefix sums (PLAN §2).
//!
//! A 100k-line transcript scrolls for free because hit-testing the viewport
//! is two binary searches over [`ScrollbackCache::visible_range`], not a
//! layout pass. Heights are measured by the view (which owns the `Context`)
//! and handed in via [`ScrollbackCache::set_height`]; this struct only does
//! arithmetic, so tests never need a GPU.
//!
//! Streaming mutates only the tail block ([`ScrollbackCache::replace_tail`]).
//! A width change (window resize) flags every stored height as stale
//! ([`ScrollbackCache::take_stale`]); the view then re-measures the visible
//! blocks first and the rest lazily.

use std::ops::Range;

use super::blocks::ContentBlock;

/// Laid-out heights and their prefix sums for one session's transcript.
#[derive(Clone, Debug, Default)]
pub struct ScrollbackCache {
    blocks: Vec<ContentBlock>,
    heights: Vec<f32>,
    /// `prefix[0] == 0`, `prefix[i + 1] == prefix[i] + heights[i]`.
    prefix: Vec<f32>,
    width: f32,
    stale: bool,
}

impl ScrollbackCache {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            heights: Vec::new(),
            prefix: vec![0.0],
            width: 0.0,
            stale: false,
        }
    }

    /// Append a block; its height starts unmeasured (`0.0`) until the view
    /// reports it via [`ScrollbackCache::set_height`].
    pub fn push(&mut self, block: ContentBlock) {
        self.blocks.push(block);
        self.heights.push(0.0);
        self.prefix.push(self.total_height());
    }

    /// Replace the tail block while streaming; its height resets to unmeasured.
    pub fn replace_tail(&mut self, block: ContentBlock) {
        if self.blocks.is_empty() {
            self.push(block);
            return;
        }
        let tail = self.blocks.len() - 1;
        self.blocks[tail] = block;
        self.heights[tail] = 0.0;
        self.rebuild_from(tail);
    }

    /// Record a measured height (points, clamped to `>= 0`).
    pub fn set_height(&mut self, index: usize, height: f32) {
        if let Some(slot) = self.heights.get_mut(index) {
            *slot = height.max(0.0);
            self.rebuild_from(index);
        }
    }

    /// A new column width invalidates every stored height lazily: values are
    /// kept (so the scrollbar does not jump) but flagged stale until the
    /// view re-measures, visible blocks first.
    pub fn set_width(&mut self, width: f32) {
        if width != self.width {
            self.width = width;
            self.stale = true;
        }
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn is_stale(&self) -> bool {
        self.stale
    }

    /// Take the stale flag (the view calls this, then re-measures).
    pub fn take_stale(&mut self) -> bool {
        std::mem::replace(&mut self.stale, false)
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&ContentBlock> {
        self.blocks.get(index)
    }

    pub fn height(&self, index: usize) -> Option<f32> {
        self.heights.get(index).copied()
    }

    pub fn total_height(&self) -> f32 {
        self.prefix.last().copied().unwrap_or(0.0)
    }

    /// Block indices overlapping `[top, bottom)`; two binary searches.
    pub fn visible_range(&self, top: f32, bottom: f32) -> Range<usize> {
        if self.blocks.is_empty() || bottom <= 0.0 || top >= self.total_height() {
            return 0..0;
        }
        let top = top.max(0.0);
        let start = self
            .prefix
            .partition_point(|&edge| edge <= top)
            .saturating_sub(1);
        let end = self
            .prefix
            .partition_point(|&edge| edge < bottom)
            .min(self.blocks.len());
        start..end.max(start)
    }

    fn rebuild_from(&mut self, from: usize) {
        for i in from..self.heights.len() {
            self.prefix[i + 1] = self.prefix[i] + self.heights[i];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::blocks::ContentBlock;
    use epaint::text::LayoutJob;

    fn prose(text: &str) -> ContentBlock {
        ContentBlock::Prose(LayoutJob::single_section(
            text.to_owned(),
            Default::default(),
        ))
    }

    fn cache_with(heights: &[f32]) -> ScrollbackCache {
        let mut cache = ScrollbackCache::new();
        for (i, &height) in heights.iter().enumerate() {
            cache.push(prose(&format!("block {i}")));
            cache.set_height(i, height);
        }
        cache
    }

    #[test]
    fn transcript_prefix_sums_track_heights() {
        let cache = cache_with(&[10.0, 20.0, 30.0]);
        assert_eq!(cache.len(), 3);
        assert_eq!(cache.total_height(), 60.0);
        assert_eq!(cache.height(1), Some(20.0));
        assert_eq!(cache.height(9), None);
    }

    #[test]
    fn transcript_mid_edit_keeps_suffix_sums() {
        let mut cache = cache_with(&[10.0, 20.0, 30.0]);
        cache.set_height(0, 15.0);
        assert_eq!(cache.total_height(), 65.0);
        assert_eq!(cache.visible_range(14.0, 16.0), 0..2);
        assert_eq!(cache.visible_range(15.0, 16.0), 1..2);
        // Out-of-range writes are ignored, never panic.
        cache.set_height(9, 5.0);
        assert_eq!(cache.total_height(), 65.0);
    }

    #[test]
    fn transcript_tail_mutation_resets_tail_height() {
        let mut cache = cache_with(&[10.0, 20.0]);
        cache.replace_tail(prose("streaming…"));
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.height(1), Some(0.0));
        assert_eq!(cache.total_height(), 10.0);
        assert_eq!(
            cache.get(1).and_then(ContentBlock::prose_text),
            Some("streaming…")
        );
        cache.set_height(1, 25.0);
        assert_eq!(cache.total_height(), 35.0);
        // Replacing the tail of an empty cache pushes.
        let mut empty = ScrollbackCache::new();
        empty.replace_tail(prose("first"));
        assert_eq!(empty.len(), 1);
    }

    #[test]
    fn transcript_width_change_flags_stale_heights() {
        let mut cache = cache_with(&[10.0]);
        assert!(!cache.is_stale());
        cache.set_width(700.0);
        assert!(cache.is_stale());
        // Same width again: no spurious invalidation.
        assert!(cache.take_stale());
        assert!(!cache.is_stale());
        cache.set_width(700.0);
        assert!(!cache.is_stale());
    }

    #[test]
    fn transcript_viewport_maps_to_block_range() {
        // Blocks span [0,10), [10,30), [30,60).
        let cache = cache_with(&[10.0, 20.0, 30.0]);
        assert_eq!(cache.visible_range(0.0, 60.0), 0..3);
        assert_eq!(cache.visible_range(0.0, 5.0), 0..1);
        assert_eq!(cache.visible_range(5.0, 35.0), 0..3);
        assert_eq!(cache.visible_range(15.0, 25.0), 1..2);
        assert_eq!(cache.visible_range(30.0, 31.0), 2..3);
        assert_eq!(cache.visible_range(-50.0, 5.0), 0..1);
        assert_eq!(cache.visible_range(100.0, 200.0), 0..0);
        assert_eq!(cache.visible_range(0.0, 0.0), 0..0);
        assert!(ScrollbackCache::new().visible_range(0.0, 600.0).is_empty());
    }

    #[test]
    fn transcript_new_blocks_start_unmeasured() {
        let mut cache = ScrollbackCache::new();
        assert!(cache.is_empty());
        cache.push(prose("fresh"));
        assert_eq!(cache.height(0), Some(0.0));
        assert_eq!(cache.total_height(), 0.0);
    }
}
