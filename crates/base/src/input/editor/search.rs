use crate::input::InputModeKind;
use aho_corasick::AhoCorasick;
use gpui::{Context, Window};
use regex::{Regex, RegexBuilder};
use ropey::Rope;
use std::{ops::Range, rc::Rc};

use super::{
    InputBaseState, Replace, RopeExt as _, Search, movement::MoveDirection, state::ScrollPadding,
};

#[derive(Debug, Clone)]
pub struct SearchMatcher {
    text: Rope,
    pub query: Option<AhoCorasick>,
    regex_query: Option<Regex>,
    matched_ranges: Rc<Vec<Range<usize>>>,
    current_match_ix: usize,
    replacing: bool,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SearchSession {
    pub open: bool,
    pub replace_mode: bool,
    pub case_insensitive: bool,
    pub query: String,
    pub replacement: String,
    pub anchor_offset: Option<usize>,
    pub matcher: SearchMatcher,

    active: bool,
    regex_mode: bool,
    preserve_case: bool,
}

impl Default for SearchSession {
    fn default() -> Self {
        Self {
            open: false,
            active: false,
            regex_mode: false,
            preserve_case: false,
            replace_mode: false,
            case_insensitive: true,
            query: String::new(),
            replacement: String::new(),
            anchor_offset: None,
            matcher: SearchMatcher::new(),
        }
    }
}

impl SearchSession {
    pub(crate) fn open(&mut self, replace_mode: bool, replaceable: bool) {
        self.open = true;
        self.active = true;
        self.replace_mode = replace_mode && replaceable;
    }

    pub(crate) fn activate(&mut self) {
        self.active = true;
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.active = false;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub(crate) fn update_query(&mut self, query: impl Into<String>, case_insensitive: bool) {
        let query = query.into();
        if !self.regex_mode && self.query == query && self.case_insensitive == case_insensitive {
            return;
        }

        self.query = query;
        self.regex_mode = false;
        self.case_insensitive = case_insensitive;
        self.matcher.update_query(&self.query, case_insensitive);
    }
}

impl<M: InputModeKind> InputBaseState<M> {
    pub fn set_search_match_background(
        &mut self,
        color: Option<gpui::Hsla>,
        cx: &mut Context<Self>,
    ) {
        self.search_match_background = color;
        cx.notify();
    }

    pub fn open_search(&mut self, replace_mode: bool, cx: &mut Context<Self>) {
        if !self.searchable {
            return;
        }
        self.search_activation_revision = self.search_activation_revision.wrapping_add(1);
        self.search_session
            .open(replace_mode, self.is_replaceable());
        let selected = self.selected_text().to_string();
        let query = if selected.is_empty() {
            self.search_session.query.clone()
        } else {
            selected
        };
        let query_changed = query != self.search_session.query;
        // A retained query resumes its previous occurrence. Only a new query
        // is anchored to the current viewport.
        self.search_session.anchor_offset = if query_changed {
            self.last_layout
                .as_ref()
                .map(|layout| layout.visible_range_offset.start)
        } else {
            None
        };
        let case_insensitive = self.search_session.case_insensitive;
        self.search_session.update_query(query, case_insensitive);
        self.search_session.matcher.update(&self.text);
        if query_changed && let Some(anchor) = self.search_session.anchor_offset {
            self.search_session.matcher.update_cursor_by_offset(anchor);
        }
        cx.notify();
    }

    pub fn search_session(&self) -> &SearchSession {
        &self.search_session
    }

    pub fn search_activation_revision(&self) -> u64 {
        self.search_activation_revision
    }

    #[doc(hidden)]
    pub fn set_search_replace_mode(&mut self, replace_mode: bool, cx: &mut Context<Self>) {
        self.search_session.replace_mode = replace_mode && self.is_replaceable();
        cx.notify();
    }

    pub fn is_replaceable(&self) -> bool {
        self.replaceable && self.is_editable()
    }

    pub fn set_search_query(
        &mut self,
        query: impl Into<String>,
        case_insensitive: bool,
        cx: &mut Context<Self>,
    ) {
        self.search_session.activate();
        self.search_session.update_query(query, case_insensitive);
        self.search_session.matcher.update(&self.text);
        cx.notify();
    }

    pub fn set_search_regex(
        &mut self,
        query: impl Into<String>,
        case_insensitive: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), regex::Error> {
        let query = query.into();
        let compiled = if query.is_empty() {
            Ok(None)
        } else {
            RegexBuilder::new(&query)
                .case_insensitive(case_insensitive)
                .multi_line(true)
                .build()
                .map(Some)
        };
        let session = &mut self.search_session;
        session.activate();
        session.query = query;
        session.case_insensitive = case_insensitive;
        session.regex_mode = true;
        session.matcher.query = None;
        session.matcher.regex_query = compiled.as_ref().ok().and_then(Clone::clone);
        session.matcher.text = self.text.clone();
        session.matcher.update_matches();
        cx.notify();
        compiled.map(|_| ())
    }

    pub fn set_search_preserve_case(&mut self, preserve: bool, cx: &mut Context<Self>) {
        self.search_session.preserve_case = preserve;
        cx.notify();
    }

    pub fn close_search(&mut self, cx: &mut Context<Self>) {
        self.search_session.close();
        cx.notify();
    }

    pub fn next_search_match(&mut self, cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.sync_search_matcher();
        let range = self.search_session.matcher.next()?;
        // Match order does not describe viewport direction after a manual
        // scroll. Always allow search navigation to reveal the active match.
        self.scroll_to_with_padding(range.end, None, ScrollPadding::SurroundingLines, cx);
        Some(range)
    }

    pub fn previous_search_match(&mut self, cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.sync_search_matcher();
        let range = self.search_session.matcher.next_back()?;
        // Match order does not describe viewport direction after a manual
        // scroll. Always allow search navigation to reveal the active match.
        self.scroll_to_with_padding(range.start, None, ScrollPadding::SurroundingLines, cx);
        Some(range)
    }

    pub fn replace_current_search_match(
        &mut self,
        replacement: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_replaceable() {
            return false;
        }
        self.sync_search_matcher();
        let matcher = &mut self.search_session.matcher;
        let Some(range) = matcher
            .matched_ranges()
            .get(matcher.current_match_index())
            .cloned()
        else {
            return false;
        };
        let source = matcher
            .regex_query
            .as_ref()
            .map(|_| self.text.to_string())
            .unwrap_or_default();
        let replacement = matcher.replacement_text(
            &source,
            &range,
            replacement,
            self.search_session.preserve_case,
        );
        let next = matcher.peek().unwrap_or_else(|| range.clone());
        let direction = matcher
            .has_next_without_wrap()
            .then_some(MoveDirection::Down);
        if direction.is_none() {
            matcher.set_current_match_index(0);
        }
        matcher.begin_replacement();
        let range_utf16 = self.range_to_utf16(&range);
        self.scroll_to(next.end, direction, cx);
        self.replace_text_in_range_silent(Some(range_utf16), &replacement, window, cx);
        true
    }

    pub fn replace_all_search_matches(
        &mut self,
        replacement: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        if !self.is_replaceable() {
            return 0;
        }
        self.sync_search_matcher();
        let ranges = self.search_session.matcher.matched_ranges();
        if ranges.is_empty() {
            return 0;
        }
        let mut text = self.text.clone();
        let source = self
            .search_session
            .matcher
            .regex_query
            .as_ref()
            .map(|_| self.text.to_string())
            .unwrap_or_default();
        for range in ranges.iter().rev() {
            let replacement = self.search_session.matcher.replacement_text(
                &source,
                range,
                replacement,
                self.search_session.preserve_case,
            );
            text.replace(range.clone(), &replacement);
        }
        self.search_session.matcher.begin_replacement();
        let count = ranges.len();
        let range_utf16 = self.range_to_utf16(&(0..self.text.len()));
        self.replace_text_in_range_silent(Some(range_utf16), &text.to_string(), window, cx);
        self.scroll_to(0, Some(MoveDirection::Down), cx);
        count
    }

    pub(super) fn update_search(&mut self, _cx: &mut gpui::App) {
        if !self.search_session.is_active() {
            return;
        }
        self.sync_search_matcher();
    }

    fn sync_search_matcher(&mut self) {
        self.search_session.matcher.update(&self.text);
    }

    pub(super) fn on_action_search(&mut self, _: &Search, _: &mut Window, cx: &mut Context<Self>) {
        if !self.searchable {
            cx.propagate();
            return;
        }
        self.open_search(false, cx);
    }

    pub(super) fn on_action_replace(
        &mut self,
        _: &Replace,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.searchable {
            cx.propagate();
            return;
        }
        self.open_search(true, cx);
    }
}

impl Default for SearchMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchMatcher {
    pub fn new() -> Self {
        Self {
            text: "".into(),
            query: None,
            regex_query: None,
            matched_ranges: Rc::new(Vec::new()),
            current_match_ix: 0,
            replacing: false,
        }
    }

    pub fn update(&mut self, text: &Rope) {
        if self.text.eq(text) {
            self.replacing = false;
            return;
        }
        self.text = text.clone();
        self.update_matches();
    }

    pub fn update_query(&mut self, query: &str, case_insensitive: bool) {
        self.regex_query = None;
        self.query = (!query.is_empty()).then(|| {
            AhoCorasick::builder()
                .ascii_case_insensitive(case_insensitive)
                .build([query])
                .expect("failed to build input search query")
        });
        self.update_matches();
    }

    pub fn matched_ranges(&self) -> Rc<Vec<Range<usize>>> {
        self.matched_ranges.clone()
    }

    pub fn current_match_index(&self) -> usize {
        self.current_match_ix
    }

    pub fn current(&self) -> Option<usize> {
        (!self.is_empty()).then_some(self.current_match_ix)
    }

    pub fn len(&self) -> usize {
        self.matched_ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.matched_ranges.is_empty()
    }

    pub fn label(&self) -> String {
        match self.current() {
            Some(ix) => format!("{}/{}", ix + 1, self.len()),
            None => "0/0".into(),
        }
    }

    fn peek(&self) -> Option<Range<usize>> {
        self.next_index()
            .and_then(|ix| self.matched_ranges.get(ix).cloned())
    }

    fn has_next_without_wrap(&self) -> bool {
        self.current_match_ix < self.matched_ranges.len().saturating_sub(1)
    }

    pub fn update_cursor_by_offset(&mut self, offset: usize) {
        for (ix, range) in self.matched_ranges.iter().enumerate() {
            self.current_match_ix = ix;
            if range.contains(&offset) || range.end >= offset {
                return;
            }
        }
    }

    fn begin_replacement(&mut self) {
        self.replacing = true;
    }

    fn set_current_match_index(&mut self, index: usize) {
        self.current_match_ix = index.min(self.matched_ranges.len().saturating_sub(1));
    }

    fn next_index(&self) -> Option<usize> {
        if self.is_empty() {
            None
        } else if self.has_next_without_wrap() {
            Some(self.current_match_ix + 1)
        } else {
            Some(0)
        }
    }

    fn update_matches(&mut self) {
        let mut ranges = Vec::new();
        if let Some(query) = &self.regex_query {
            let text = self.text.to_string();
            ranges.extend(query.find_iter(&text).map(|matched| matched.range()));
        } else if let Some(query) = &self.query {
            let text = self.text.to_string();
            ranges.extend(
                query
                    .stream_find_iter(text.as_bytes())
                    .map(|result| result.expect("input search match").range()),
            );
        }
        self.matched_ranges = Rc::new(ranges);
        if !self.replacing || self.is_empty() {
            self.current_match_ix = 0;
        } else {
            self.current_match_ix = self.current_match_ix.min(self.len() - 1);
        }
        self.replacing = false;
    }

    fn replacement_text(
        &self,
        source: &str,
        range: &Range<usize>,
        template: &str,
        preserve: bool,
    ) -> String {
        let replacement = if let Some(query) = &self.regex_query {
            let mut expanded = String::new();
            if let Some(captures) = query.captures_at(source, range.start) {
                captures.expand(template, &mut expanded);
            }
            expanded
        } else {
            template.to_owned()
        };
        if preserve {
            preserve_match_case(&self.text.slice(range.clone()).to_string(), replacement)
        } else {
            replacement
        }
    }
}

fn preserve_match_case(matched: &str, replacement: String) -> String {
    let mut letters = matched
        .chars()
        .filter(|c| c.is_uppercase() || c.is_lowercase());
    let Some(first) = letters.next() else {
        return replacement;
    };
    if first.is_lowercase() && letters.clone().all(char::is_lowercase) {
        replacement.to_lowercase()
    } else if first.is_uppercase() && letters.clone().all(char::is_uppercase) {
        replacement.to_uppercase()
    } else if first.is_uppercase() && letters.all(char::is_lowercase) {
        let lower = replacement.to_lowercase();
        let Some((offset, letter)) = lower.char_indices().find(|(_, c)| c.is_lowercase()) else {
            return lower;
        };
        format!(
            "{}{}{}",
            &lower[..offset],
            letter.to_uppercase(),
            &lower[offset + letter.len_utf8()..]
        )
    } else {
        replacement
    }
}

impl Iterator for SearchMatcher {
    type Item = Range<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        let ix = self.next_index()?;
        self.current_match_ix = ix;
        self.matched_ranges.get(ix).cloned()
    }
}

impl DoubleEndedIterator for SearchMatcher {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.is_empty() {
            return None;
        }
        if self.current_match_ix == 0 {
            self.current_match_ix = self.len();
        }
        self.current_match_ix -= 1;
        self.matched_ranges.get(self.current_match_ix).cloned()
    }
}
