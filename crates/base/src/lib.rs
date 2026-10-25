//! Behavior and infrastructure foundations for GPUI applications.
//!
//! Primitives deliberately avoid presentation styles. Layout, positioning,
//! colors, sizing, and motion belong to applications or the
//! `gpui-component` façade.

#[cfg(feature = "gpui-fast")]
extern crate gpui_fast as gpui;
#[cfg(all(feature = "gpui-fast", test))]
extern crate gpui_fast_platform as gpui_platform;

pub mod actions;
pub mod animation;
#[doc(hidden)]
pub mod async_util;
mod auto_scroll;
mod button;
mod checkbox;
pub mod component_traits;
mod element_ext;
mod event;
mod focus_trap;
mod geometry;
mod global_state;
mod history;
mod index_path;
pub mod input;
mod link;
mod list_settings;
mod measure;
pub mod motion;
mod number_input;
mod observe;
mod otp_input;
mod popover;
mod popup;
mod positioner;
mod radio;
mod radio_group;
mod reduce_motion;
mod root;
mod scroll_bounce;
mod scrollable_mask;
mod scrollbar;
mod selectable_text;
mod state_style;
mod styled;
pub mod text;
mod text_boundary;
mod text_selection;
mod theme;
pub mod theme_tokens;
mod toolbar;
mod tooltip;
mod touch_selection;
mod undo_history;
mod virtual_list;

pub use auto_scroll::AutoScroll;
pub use button::{Button, ButtonStyles};
pub use checkbox::{
    Checkbox, CheckboxIndicator, CheckboxIndicatorStyles, CheckboxState, CheckboxStyles,
};
pub use component_traits::FocusableExt;
pub use component_traits::{Disableable, Selectable};
pub use element_ext::ElementExt;
pub use event::{InteractiveElementExt, OngoingScrollExt};
pub use focus_trap::FocusTrapElement;
#[doc(hidden)]
pub use focus_trap::active_focus_trap;
pub use geometry::*;
pub use global_state::{DeferredPopover, GlobalState};
pub use history::History;
pub use index_path::IndexPath;
pub use input::{Editor, Input, InputBase, InputStyles, Textarea};
pub use link::{Link, LinkStyles};
pub use list_settings::ListSettings;
#[doc(hidden)]
pub use measure::measurement_enabled;
pub use measure::{Measure, measure, measure_if};
pub use motion::{
    Discrete, DiscreteError, Easing, EasingError, Interpolate, IterationCount, Keyframe,
    KeyframeError, Keyframes, LinearStop, MotionPhase, MotionReveal, MotionStatus, MotionTransform,
    MotionValue, PlaybackDirection, Presence, PresencePhase, PresenceSample, Sequence,
    SequenceSample, SequenceStep, SignedDuration, Spring, SpringError, Stagger, StaggerOrigin,
    StepPosition, Timing, TimingSample, Transition, TransitionId, animate_keyframes, spring,
    transition, transition_with_status,
};
pub use number_input::{
    Decrement, Increment, NumberInput, NumberInputEvent, NumberInputText, NumberStep, StepAction,
    step_value,
};
pub use observe::{ObservedElement, TestSupportExt};
pub use otp_input::{OtpEvent, OtpInput, OtpState};
pub use popover::{Popover, PopoverState};
pub use popup::{POPUP_PRIORITY, Popup};
pub use positioner::{Align, Positioner, ResolvedPosition};
pub use radio::{Radio, RadioStyles};
pub use radio_group::RadioGroup;
pub use reduce_motion::apply_system_reduce_motion;
pub use root::{Root, RootPlugin};
pub use scroll_bounce::{ScrollBounce, ScrollBounceMotion};
pub use scrollable_mask::{RoundedFrameCover, ScrollableMask};
pub use scrollbar::{
    Scrollbar, ScrollbarAxis, ScrollbarEntrance, ScrollbarHandle, ScrollbarMode, ScrollbarMotion,
    ScrollbarStyles, ScrollbarThumbStyle, ScrollbarTrackStyle,
};
pub use selectable_text::SelectableText;
pub use state_style::StateStyle;
#[cfg(any(feature = "inspector", debug_assertions))]
pub use styled::styled_ext_reflection_methods;
pub use styled::{RoleOverride, StyledExt, box_shadow, h_flex, v_flex};
pub use text::{
    InlineElement, InlineRenderContext, MarkdownExtensions, MarkdownNode, MarkdownParseContext,
    MarkdownPlugin, RangeHighlight, RangeHighlightError, RenderedText, SelectionFormat, TableData,
    Text, TextView, TextViewDefaults, TextViewMotion, TextViewPlugin, TextViewState, TextViewStyle,
    html, markdown, markdown_ast,
};
pub use text_selection::{
    TextSelection, TextSelectionContentKey, TextSelectionCoverage, TextSelectionEndpoint,
    TextSelectionEvent, TextSelectionHandle, TextSelectionLayer, TextSelectionProjection,
    TextSelectionRegistration, TextSelectionRun, TextSelectionScopeId, TextSelectionSnapshot,
    TextSelectionWindowPoints, TouchHandleLayout,
};
pub use theme::{ResizableTheme, ScrollbarTheme, Theme, ThemeAppearance};
pub use theme_tokens::{
    ColorTokens, RadiusTokens, SemanticThemeTokens, ShadowTokens, SpacingTokens, TextStyleToken,
    TypographyTokens,
};
pub use toolbar::{Toolbar, ToolbarGroup};
pub use tooltip::{
    Tooltip, TooltipDefaults, TooltipOverlay, TooltipPositioner, TooltipRequest, TooltipTransition,
};
pub use touch_selection::{SelectionEdge, TouchHandle, TouchSelectionSnapshot};
pub use undo_history::UndoHistory;
#[doc(hidden)]
pub use virtual_list::virtual_list;
pub use virtual_list::{VirtualList, VirtualListScrollHandle, h_virtual_list, v_virtual_list};

use gpui::App;
pub const fn is_mobile() -> bool {
    cfg!(any(target_os = "ios", target_os = "android"))
}
pub fn init(cx: &mut App) {
    let _ = Theme::global_mut(cx);
    GlobalState::init(cx);
    reduce_motion::init(cx);
    focus_trap::init(cx);
    popover::init(cx);
    number_input::init(cx);
    input::init(cx);
    text::init(cx);
    root::init(cx);
}
