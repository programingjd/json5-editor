use gpui::{
    App, IntoElement, ListState, ParentElement as _, SharedString, Styled as _, Window, div,
};

use std::{
    ops::{Range, RangeInclusive},
    sync::Arc,
};

use crate::text::{
    SelectionFormat,
    node::{BlockNode, NodeContext, SourceRangeSelection},
};

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct ParsedDocument {
    pub(crate) source: SharedString,
    pub(crate) blocks: Arc<Vec<BlockNode>>,
}

#[derive(Default, Clone, Copy)]
pub(crate) struct NodeRenderOptions {
    pub(crate) ix: usize,
    pub(crate) in_list: bool,
    pub(crate) todo: bool,
    pub(crate) ordered: bool,
    pub(crate) list_start: Option<u32>,
    pub(crate) depth: usize,
    pub(crate) is_last: bool,

    pub(crate) is_first: bool,

    pub(crate) prev: PrevBlock,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PrevBlock {
    #[default]
    Other,

    Heading(u8),
    Rule,
}

pub(crate) fn flow_position(blocks: &[BlockNode], ix: usize) -> (bool, PrevBlock) {
    let prev = blocks[..ix].iter().rev().find(|block| {
        !matches!(
            block,
            BlockNode::Definition { .. } | BlockNode::Break { .. } | BlockNode::Unknown
        )
    });
    match prev {
        None => (true, PrevBlock::Other),
        Some(BlockNode::Heading { level, .. }) => (false, PrevBlock::Heading(*level)),
        Some(BlockNode::HorizontalRule { .. }) => (false, PrevBlock::Rule),
        Some(_) => (false, PrevBlock::Other),
    }
}

impl NodeRenderOptions {
    pub(crate) fn is_last(mut self, is_last: bool) -> Self {
        self.is_last = is_last;
        self
    }
}

impl ParsedDocument {
    pub(super) fn text(&self) -> String {
        let mut text = String::new();
        for block in self.blocks.iter() {
            text.push_str(&block.text());
        }
        text
    }

    pub(super) fn selected_text(
        &self,
        format: SelectionFormat,
        blocks: Option<RangeInclusive<usize>>,
    ) -> String {
        let requested_blocks = blocks.clone();
        let painted = self
            .blocks
            .iter()
            .map(|block| block.has_selection())
            .collect::<Vec<_>>();
        let (Some(painted_first), Some(painted_last)) = (
            painted.iter().position(|painted| *painted),
            painted.iter().rposition(|painted| *painted),
        ) else {
            return String::new();
        };

        let last_ix = self.blocks.len().saturating_sub(1);
        let (first, last) = match blocks {
            Some(blocks) => (*blocks.start().min(&last_ix), *blocks.end().min(&last_ix)),
            None => (painted_first, painted_last),
        };

        if format == SelectionFormat::Plain {
            let mut text = String::new();
            for (ix, block) in self.blocks.iter().enumerate().take(last + 1).skip(first) {
                let selected = block.selected_text(format);
                let is_virtual_endpoint = requested_blocks
                    .as_ref()
                    .is_some_and(|blocks| ix == *blocks.start() || ix == *blocks.end());
                if requested_blocks.is_some() && !is_virtual_endpoint {
                    text.push_str(&block.text());
                } else if !selected.is_empty() {
                    text.push_str(&selected);
                } else if !painted[ix] {
                    // Never painted, so it cannot report a selection of its own
                    // even though the span covers it. A painted block that came
                    // up empty really has nothing selected, and stays empty.
                    text.push_str(&block.text());
                }
            }
            return text;
        }

        let mut out: Vec<String> = Vec::new();
        for (ix, block) in self.blocks.iter().enumerate().take(last + 1).skip(first) {
            // The selection is one continuous range, so only the block it
            // starts in and the block it ends in can be partly selected.
            // Everything between them is covered whole, and so is any block
            // that reports nothing — it either scrolled past without painting,
            // or renders no selectable text run at all (a rule, a break, a
            // custom node, a standalone image).
            let source = if (ix == first || ix == last) && painted[ix] {
                block.selected_text(format)
            } else {
                self.whole_source(block)
            };

            let trimmed = source.trim_end_matches('\n');
            if !trimmed.is_empty() {
                out.push(trimmed.to_string());
            }
        }
        out.join("\n\n")
    }

    fn whole_source(&self, block: &BlockNode) -> String {
        if let Some(span) = block.span()
            && let Some(source) = self.source.get(span.start..span.end)
        {
            return source.to_string();
        }

        block.selected_text(SelectionFormat::Source)
    }

    pub(super) fn selected_source_range(&self) -> Option<Range<usize>> {
        let mut selected = SourceRangeSelection::Unselected;
        for block in self.blocks.iter() {
            selected.merge(block.selected_source_range());
        }
        selected.into_range()
    }

    pub(super) fn clear_selection(&self) {
        for block in self.blocks.iter() {
            block.clear_selection();
        }
    }

    #[allow(dead_code)]
    pub(crate) fn to_markdown(&self) -> String {
        self.blocks
            .iter()
            .map(|child| child.to_markdown())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub(super) fn render_root(
        &self,
        list_state: Option<ListState>,
        node_cx: &NodeContext,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        let Some(list_state) = list_state else {
            let blocks_len = self.blocks.len();
            return div().children(self.blocks.iter().enumerate().map(move |(ix, node)| {
                let is_last = ix + 1 == blocks_len;
                let (is_first, prev) = flow_position(&self.blocks, ix);
                node.render_block(
                    NodeRenderOptions {
                        ix,
                        is_last,
                        is_first,
                        prev,
                        ..Default::default()
                    },
                    node_cx,
                    window,
                    cx,
                )
            }));
        };

        let options = NodeRenderOptions {
            is_last: true,
            ..Default::default()
        };

        let blocks = &self.blocks;
        if list_state.item_count() != blocks.len() {
            list_state.reset(blocks.len());
        }

        div().size_full().child(
            gpui::list(list_state, {
                let node_cx = node_cx.clone();
                let blocks = blocks.clone();
                move |ix, window, cx| {
                    let is_last = ix + 1 == blocks.len();
                    let (is_first, prev) = flow_position(&blocks, ix);
                    blocks[ix]
                        .render_block(
                            NodeRenderOptions {
                                ix,
                                is_last,
                                is_first,
                                prev,
                                ..options
                            },
                            &node_cx,
                            window,
                            cx,
                        )
                        .into_any_element()
                }
            })
            .size_full(),
        )
    }
}
