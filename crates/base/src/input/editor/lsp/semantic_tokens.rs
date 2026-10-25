use std::ops::Range;

use anyhow::Result;
use gpui::{App, Context, HighlightStyle, SharedString, Task, Window};
use instant::Duration;
use lsp_types::{Position, SemanticTokens, SemanticTokensLegend};
use ropey::Rope;

use crate::input::{EditorMode, HighlightStyleResolver, InputBaseState, Lsp, RopeExt};

pub trait DocumentRangeSemanticTokensProvider {
    fn legend(&self) -> SemanticTokensLegend;

    fn semantic_tokens(
        &self,
        text: &Rope,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<SemanticTokens>>;
}

impl Lsp {
    pub(crate) fn semantic_tokens_for_range(
        &self,
        text: &Rope,
        visible_range: &Range<usize>,
        theme: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        if self.semantic_tokens.is_empty() {
            return Vec::new();
        }

        let visible_start = text.offset_to_position(visible_range.start);
        let visible_end = text.offset_to_position(visible_range.end);

        // Cache is sorted by `range.start`. A token can only touch the
        // viewport if its start is before `visible_end` (upper bound) and it
        // is not on a line entirely above the viewport's first line (lower
        // bound — tokens are single-line, so an earlier line cannot reach in).
        let hi = self
            .semantic_tokens
            .partition_point(|(range, _)| range.start < visible_end);
        let lo = self
            .semantic_tokens
            .partition_point(|(range, _)| range.start.line < visible_start.line);

        self.semantic_tokens[lo..hi]
            .iter()
            .filter_map(|(range, name)| {
                let start = text.position_to_offset(&range.start);
                let end = text.position_to_offset(&range.end);
                if start >= end || start >= visible_range.end || end <= visible_range.start {
                    return None;
                }

                let style = theme.style(name.as_ref())?;
                Some((start..end, style))
            })
            .collect()
    }

    pub(crate) fn update_semantic_tokens(
        &mut self,
        text: &Rope,
        window: &mut Window,
        cx: &mut Context<InputBaseState<EditorMode>>,
    ) {
        let Some(provider) = self.semantic_tokens_provider.as_ref() else {
            return;
        };

        let provider = provider.clone();
        let legend = provider.legend();
        let text = text.clone();
        // Fetch the whole document; results are cached and filtered to the
        // viewport at paint time (mirrors `update_document_colors`), so a
        // scroll never needs a refetch.
        let range = 0..text.len();
        let input_state = cx.entity();

        // debounce timer 100ms
        self._semantic_tokens_task = cx.spawn_in(window, async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;

            let task_result = cx
                .update(|window, cx| provider.semantic_tokens(&text, range, window, cx))
                .ok();

            if let Some(task) = task_result {
                if let Ok(tokens) = task.await {
                    let decoded = decode_semantic_tokens(&tokens, &legend);
                    let _ = input_state.update(cx, |input_state, cx| {
                        if decoded != input_state.extras.lsp.semantic_tokens {
                            input_state.extras.lsp.semantic_tokens = decoded;
                            cx.notify();
                        }
                    });
                }
            }
        });
    }
}

fn decode_semantic_tokens(
    tokens: &SemanticTokens,
    legend: &SemanticTokensLegend,
) -> Vec<(lsp_types::Range, SharedString)> {
    // Resolve the legend names once; tokens then share them via cheap
    // ref-counted clones instead of allocating a String per token.
    let names: Vec<SharedString> = legend
        .token_types
        .iter()
        .map(|t| SharedString::from(t.as_str().to_owned()))
        .collect();

    let mut out = Vec::with_capacity(tokens.data.len());
    let mut line: u32 = 0;
    let mut character: u32 = 0;

    for token in &tokens.data {
        if token.delta_line > 0 {
            line += token.delta_line;
            character = token.delta_start;
        } else {
            character += token.delta_start;
        }

        let Some(name) = names.get(token.token_type as usize) else {
            continue;
        };

        let start = Position::new(line, character);
        let end = Position::new(line, character + token.length);
        out.push((lsp_types::Range { start, end }, name.clone()));
    }

    out.sort_by_key(|(range, _)| range.start);
    out
}
