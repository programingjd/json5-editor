mod clear_button;
mod content_type;
mod input;
mod token;
pub use gpui_base::input::{
    ActivateToken, InlineToken, InlineTokenClickEvent, InlineTokenContext, InlineTokenError,
    InlineTokenHoverEvent, InlineTokenSpan, InputContent,
};
pub use token::InputToken;
pub mod language_config;
mod overlay;
pub(crate) mod popovers;
mod search;
mod syntax_context;

pub(crate) use clear_button::*;
pub use content_type::*;
#[cfg(not(feature = "tree-sitter"))]
pub struct Tree;

pub(crate) use gpui_base::input::InputBaseState;
pub use gpui_base::input::{
    Backspace, BufferPoint, CompletionMenuOptions, CompletionProvider, Copy, Cut, Delete,
    DeleteToBeginningOfLine, DeleteToEndOfLine, DeleteToNextWordEnd, DeleteToPreviousWordStart,
    DisplayMap, DisplayPoint, DocumentColorProvider, DocumentRangeSemanticTokensProvider,
    EditorState, Enter, Escape, FoldRange, HighlightStyleResolver, HoverPopoverState,
    HoverProvider, Indent, IndentInline, InputEdit, InputEvent, InputHighlighter,
    InputHighlighterFactory, InputState, InputTextChange, InputTextChangeOrigin, InputTextEdit,
    Lsp, MaskPattern, MoveDown, MoveEnd, MoveHome, MoveLeft, MovePageDown, MovePageUp, MoveRight,
    MoveToEnd, MoveToEndOfLine, MoveToNextWord, MoveToPreviousWord, MoveToStart, MoveToStartOfLine,
    MoveUp, Outdent, OutdentInline, Paste, Point, PrepaintCaretGeometry, RangeDecoration,
    RangeDecorationCollection, RangeDecorationStyle, Redo, Replace, Rope, RopeExt, RopeLines,
    Search, SelectAll, SelectToEnd, SelectToEndOfLine, SelectToNextWordEnd,
    SelectToPreviousWordStart, SelectToStart, SelectToStartOfLine, Selection, ShowCharacterPalette,
    ShowCompletions, TabSize, TextDecoration, TextDecorationCollection, TextareaState, Undo,
    WrappingIndent,
};
pub use gpui_base::input::{EditorMode, InputMode, InputModeKind, TextareaMode};
#[doc(hidden)]
mod editor;
mod group;
mod state;
mod textarea;
pub use editor::Editor;
pub use gpui_base::input::{
    AutoClosingPair, BracketPair, IndentationRules, LanguageProvider, SyntaxContext,
    SyntaxContextProvider, set_language_config, set_language_provider,
};
pub use group::*;
pub use input::*;
pub use lsp_types::Position;
pub use state::AnyInputState;
pub use textarea::Textarea;

pub(crate) fn init(cx: &mut gpui::App) {
    language_config::init(cx);
}
