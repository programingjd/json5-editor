//! Compile-time input modes.
//!
//! The three input states are one engine seen through a mode marker, one per
//! state:
//!
//! ```ignore
//! pub type InputState    = InputBaseState<InputMode>;
//! pub type TextareaState = InputBaseState<TextareaMode>;
//! pub type EditorState   = InputBaseState<EditorMode>;
//! ```
//!
//! A method that only makes sense for one mode lives in that mode's `impl`
//! block, so it does not exist on the others: `InputState` has no `auto_grow`
//! or `soft_wrap`, `TextareaState` has no `masked` or `line_number`, and only
//! `EditorState` provides syntax highlighting. Methods shared by the two multi-line
//! modes go on [`MultiLineMode`]. Reaching for the wrong one is a compile
//! error rather than a debug assertion.
//!
//! [`super::LayoutMode`] carries the same distinction at runtime, since the
//! engine branches on it while editing. The marker only decides which API is
//! reachable.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{Div, Entity, Stateful, Window};
use ropey::Rope;

use super::decorations::DecorationCollections;
use super::lsp::{ContextMenuContent, InlineCompletion};
use crate::input::{
    HighlightStyleResolver, InputEdit, InputHighlighter, RangeDecoration, SyntaxContext,
    TextDecoration,
};
use crate::input::{HoverPopoverState, Lsp};
use gpui::Task;

use super::InputBaseState;

pub struct InputMode;

pub struct TextareaMode;

pub struct EditorMode;

mod sealed {
    pub trait Sealed {}
}

impl sealed::Sealed for InputMode {}
impl sealed::Sealed for TextareaMode {}
impl sealed::Sealed for EditorMode {}

pub trait MultiLineMode: InputModeKind {}

impl MultiLineMode for TextareaMode {}
impl MultiLineMode for EditorMode {}

pub trait InputExtras: Default + 'static {
    fn decoration_layers(&self) -> Vec<&[TextDecoration]> {
        Vec::new()
    }

    fn range_decorations(&self, _ranges: &[std::ops::Range<usize>]) -> Vec<&RangeDecoration> {
        Vec::new()
    }

    fn semantic_token_styles(
        &self,
        _text: &Rope,
        _range: &std::ops::Range<usize>,
        _resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> {
        Vec::new()
    }

    fn document_color_swatches(
        &self,
        _text: &Rope,
        _range: &std::ops::Range<usize>,
    ) -> Vec<(std::ops::Range<usize>, gpui::Hsla)> {
        Vec::new()
    }

    fn hover_symbol_range(&self) -> Option<std::ops::Range<usize>> {
        None
    }

    fn inline_completion_item(&self) -> Option<&lsp_types::InlineCompletionItem> {
        None
    }
}

impl InputExtras for () {}

pub trait InputModeKind: sealed::Sealed + Sized + 'static {
    const MULTI_LINE: bool;

    const CODE_EDITOR: bool = false;

    type Extras: InputExtras;

    fn drive_highlighter(
        _highlighter: &Rc<RefCell<Option<Box<dyn InputHighlighter>>>>,
        _edit: InputEdit,
        _text: &Rope,
        _folding: bool,
        _window: &mut Window,
        _cx: &mut gpui::Context<InputBaseState<Self>>,
    ) {
    }

    fn drive_highlighter_batch(
        _highlighter: &Rc<RefCell<Option<Box<dyn InputHighlighter>>>>,
        _edits: &[(InputEdit, Rope)],
        _folding: bool,
        _window: &mut Window,
        _cx: &mut gpui::Context<InputBaseState<Self>>,
    ) {
    }

    fn reset_language_features(_state: &mut InputBaseState<Self>) {}

    fn reset_annotations(_state: &mut InputBaseState<Self>) {}

    fn editing_syntax_context(_state: &InputBaseState<Self>, _offset: usize) -> SyntaxContext {
        SyntaxContext::Code
    }

    fn adjust_annotations(
        _state: &mut InputBaseState<Self>,
        _range: &std::ops::Range<usize>,
        _new_len: usize,
    ) {
    }

    fn refresh_language_features(
        _state: &mut InputBaseState<Self>,
        _window: &mut Window,
        _cx: &mut gpui::Context<InputBaseState<Self>>,
    ) {
    }

    fn accept_inline_completion(
        _state: &mut InputBaseState<Self>,
        _window: &mut Window,
        _cx: &mut gpui::Context<InputBaseState<Self>>,
    ) -> bool {
        false
    }

    fn has_inline_completion(_state: &InputBaseState<Self>) -> bool {
        false
    }

    fn clear_hover_state(
        _state: &mut InputBaseState<Self>,
        _cx: &mut gpui::Context<InputBaseState<Self>>,
    ) {
    }

    fn on_text_typed(
        _state: &mut InputBaseState<Self>,
        _range: &std::ops::Range<usize>,
        _text: &str,
        _window: &mut Window,
        _cx: &mut gpui::Context<InputBaseState<Self>>,
    ) {
    }

    fn clear_inline_completion(
        _state: &mut InputBaseState<Self>,
        _cx: &mut gpui::Context<InputBaseState<Self>>,
    ) {
    }

    fn hide_context_menu(
        _state: &mut InputBaseState<Self>,
        _cx: &mut gpui::Context<InputBaseState<Self>>,
    ) {
    }

    fn is_context_menu_open(_state: &InputBaseState<Self>, _cx: &gpui::App) -> bool {
        false
    }

    fn handle_context_menu_action(
        _state: &mut InputBaseState<Self>,
        _action: Box<dyn gpui::Action>,
        _window: &mut Window,
        _cx: &mut gpui::Context<InputBaseState<Self>>,
    ) -> bool {
        false
    }

    fn on_mouse_move(
        _state: &mut InputBaseState<Self>,
        _offset: usize,
        _event: &gpui::MouseMoveEvent,
        _window: &mut Window,
        _cx: &mut gpui::Context<InputBaseState<Self>>,
    ) {
    }

    fn register_actions(
        element: Stateful<Div>,
        _entity: &Entity<InputBaseState<Self>>,
        _window: &mut Window,
    ) -> Stateful<Div> {
        element
    }
}

impl InputModeKind for InputMode {
    const MULTI_LINE: bool = false;

    type Extras = ();
}
impl InputModeKind for TextareaMode {
    const MULTI_LINE: bool = true;

    type Extras = ();
}
// `EditorMode`'s implementation lives with the editor code, next to the
// language features it dispatches to.

pub struct EditorExtras {
    pub(crate) lsp: Lsp,
    pub(crate) decorations: DecorationCollections,
    pub(crate) range_decorations: DecorationCollections<RangeDecoration>,
    pub(crate) inline_completion: InlineCompletion,
    pub(crate) context_menu_content: ContextMenuContent,
    pub(crate) hover_popover: Option<HoverPopoverState>,
    pub(crate) context_menu_task: Task<anyhow::Result<()>>,
}

impl Default for EditorExtras {
    fn default() -> Self {
        Self {
            lsp: Lsp::default(),
            decorations: DecorationCollections::default(),
            range_decorations: DecorationCollections::default(),
            inline_completion: InlineCompletion::default(),
            context_menu_content: ContextMenuContent::default(),
            hover_popover: None,
            context_menu_task: Task::ready(Ok(())),
        }
    }
}
