//! Boundary displacement only: the list keeps its clamped logical position.

use crate::{OngoingScrollExt as _, ScrollbarHandle};
use gpui::{
    AnyElement, App, Bounds, ContentMask, DispatchPhase, Element, ElementId, GlobalElementId,
    Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId, OngoingScroll, Pixels,
    ScrollDelta, ScrollWheelEvent, TouchPhase, Window, point, px,
};
#[cfg(not(target_family = "wasm"))]
use std::time::Instant;
use std::{cell::RefCell, rc::Rc, time::Duration};
#[cfg(target_family = "wasm")]
use web_time::Instant;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollBounceMotion {
    tracking: f32,
    response: Duration,
}

impl Default for ScrollBounceMotion {
    fn default() -> Self {
        Self {
            tracking: 0.55,
            response: Duration::from_millis(524),
        }
    }
}

impl ScrollBounceMotion {
    pub fn with_tracking(mut self, tracking: f32) -> Self {
        assert!(
            tracking.is_finite() && tracking > 0.,
            "scroll bounce tracking must be finite and positive"
        );
        self.tracking = tracking;
        self
    }

    pub fn with_response(mut self, response: Duration) -> Self {
        self.response = response;
        self
    }

    pub fn tracking(&self) -> f32 {
        self.tracking
    }

    pub fn response(&self) -> Duration {
        self.response
    }

    fn omega(&self) -> Option<f32> {
        let seconds = self.response.as_secs_f32();
        (seconds > 0.).then(|| std::f32::consts::TAU / seconds)
    }
}

pub struct ScrollBounce {
    id: ElementId,
    handle: Rc<dyn ScrollbarHandle>,
    child: AnyElement,
    enabled: bool,
    motion: ScrollBounceMotion,
    on_scroll: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
}

impl ScrollBounce {
    pub fn new<H: ScrollbarHandle + Clone>(
        id: impl Into<ElementId>,
        handle: &H,
        child: impl IntoElement,
    ) -> Self {
        Self {
            id: id.into(),
            handle: Rc::new(handle.clone()),
            child: child.into_any_element(),
            enabled: cfg!(any(target_os = "ios", target_os = "android")),
            motion: ScrollBounceMotion::default(),
            on_scroll: None,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn motion(mut self, motion: ScrollBounceMotion) -> Self {
        self.motion = motion;
        self
    }

    pub fn on_scroll(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_scroll = Some(Rc::new(handler));
        self
    }
}

#[derive(Default)]
struct State {
    physics: Physics,
    sampled_at: Option<Instant>,
    ongoing_scroll: OngoingScroll,
    short_drag_distance: Option<f32>,
    last_wheel_at: Option<Instant>,

    suppressed_direction: Option<f32>,
}

impl State {
    fn release(&mut self, from_rest: bool) {
        self.physics.release();
        self.suppressed_direction =
            (from_rest && self.physics.suppress_momentum).then(|| self.physics.offset().signum());
    }
}

// GPUI starts a normal touch pan only after its 8 px touch slop, but a touch
// catching a fling starts at zero displacement. A short catch should stop the
// old fling rather than turn a few fast pixels into a new one.
const CATCH_DRAG_SLOP: f32 = 8.;

// Momentum arrives once per frame until it stops, so a longer silence means
// the suppressed stream has ended. Smooth-scrolling mouse drivers on macOS
// send precise deltas with no phase: they never send the `Started` that
// otherwise ends suppression, and would stay locked after one bounce.
const MOMENTUM_GAP: Duration = Duration::from_millis(250);

fn max_scroll_extent(handle: &dyn ScrollbarHandle) -> Pixels {
    (handle.content_size().height - handle.viewport_bounds().size.height).max(px(0.))
}

#[doc(hidden)]
pub struct ScrollBouncePrepaintState {
    state: Rc<RefCell<State>>,
    hitbox: Hitbox,
}

impl IntoElement for ScrollBounce {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for ScrollBounce {
    type RequestLayoutState = ();
    type PrepaintState = ScrollBouncePrepaintState;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let state = window.with_element_state(
            id.expect("ScrollBounce has an id"),
            |state: Option<Rc<RefCell<State>>>, _| {
                let state = state.unwrap_or_default();
                (state.clone(), state)
            },
        );
        let offset = {
            let mut state = state.borrow_mut();
            if !self.enabled || cx.reduce_motion() {
                *state = State::default();
            }
            state.physics.motion = self.motion;
            let now = Instant::now();
            let elapsed = state
                .sampled_at
                .replace(now)
                .map_or(0., |at| now.duration_since(at).as_secs_f32());
            if state.physics.step(elapsed) {
                window.request_animation_frame();
            }
            state.physics.offset()
        };
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.with_element_offset(point(px(0.), px(offset)), |window| {
                self.child.prepaint(window, cx);
            });
        });
        ScrollBouncePrepaintState { state, hitbox }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.enabled && !cx.reduce_motion() {
            let state = prepaint.state.clone();
            let hitbox = prepaint.hitbox.id;
            let handle = self.handle.clone();
            let view = window.current_view();
            let on_scroll = self.on_scroll.clone();
            let mut before = 0.;
            let mut allow_end_bounce = false;
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                let ScrollDelta::Pixels(mut delta) = event.delta else {
                    return;
                };
                if !hitbox.should_handle_scroll(window) {
                    return;
                }
                let mut state = state.borrow_mut();
                // Lock the gesture to the axis it started on, so a diagonal
                // swipe cannot wobble out of the stretch from one packet to
                // the next. Both dispatch phases see the same packet, and the
                // lock gives both the same answer.
                state
                    .ongoing_scroll
                    .lock_axis(&mut delta, event.touch_phase);
                if delta.x.abs() > delta.y.abs() {
                    return;
                }
                let ended = matches!(event.touch_phase, TouchPhase::Ended | TouchPhase::Cancelled);
                let mut scrolled = false;
                let mut changed = false;
                if phase == DispatchPhase::Capture {
                    before = handle.offset().y.as_f32();
                    if event.touch_phase == TouchPhase::Started {
                        state.short_drag_distance = (delta.y == px(0.)).then_some(0.);
                        state.physics.begin(bounds.size.height.as_f32());
                    } else if let Some(distance) = state.short_drag_distance.as_mut() {
                        *distance += delta.y.as_f32().abs();
                        if *distance > CATCH_DRAG_SLOP {
                            state.short_drag_distance = None;
                        }
                    }
                    let suppress_short_drag_momentum = if ended {
                        state.short_drag_distance.take().is_some()
                            && event.touch_phase == TouchPhase::Ended
                    } else {
                        false
                    };
                    // The current Ended packet may still cross an edge; only
                    // momentum packets after it should be suppressed.
                    allow_end_bounce = suppress_short_drag_momentum;
                    let now = cx.background_executor().now();
                    let paused = state
                        .last_wheel_at
                        .replace(now)
                        .is_some_and(|at| now.saturating_duration_since(at) >= MOMENTUM_GAP);
                    let reversed = state.suppressed_direction.is_some_and(|direction| {
                        delta.y != px(0.) && delta.y.as_f32().signum() != direction
                    });
                    if paused || reversed {
                        state.physics.suppress_momentum = false;
                    }
                    if state.physics.suppress_momentum {
                        cx.stop_propagation();
                        return;
                    }
                    if state.physics.offset() != 0. {
                        // Outside a gesture (a phaseless wheel once suppression
                        // lifts) a packet grabs the returning edge and lets it
                        // go again, as it would at rest.
                        let from_rest = !state.physics.dragging;
                        if from_rest {
                            state.physics.begin(bounds.size.height.as_f32());
                        }
                        let remainder = state.physics.pull(delta.y.as_f32());
                        if remainder != 0. {
                            let max = max_scroll_extent(handle.as_ref());
                            let mut offset = handle.offset();
                            offset.y = px(before + remainder).clamp(-max, px(0.));
                            handle.set_offset(offset);
                            scrolled = true;
                        }
                        if ended || from_rest {
                            state.release(from_rest);
                        }
                        changed = true;
                        cx.stop_propagation();
                    } else if ended {
                        state.physics.release();
                    }
                    if suppress_short_drag_momentum {
                        state.physics.suppress_momentum = true;
                        state.suppressed_direction = None;
                    }
                } else {
                    // Div applies deltas immediately but clamps during its next
                    // prepaint. Clamp here so that boundary deltas are not
                    // mistaken for consumed scrolling (ListState clamps eagerly).
                    let mut offset = handle.offset();
                    let max = max_scroll_extent(handle.as_ref());
                    let clamped = offset.y.clamp(-max, px(0.));
                    if clamped != offset.y {
                        offset.y = clamped;
                        handle.set_offset(offset);
                    }
                    let after = offset.y.as_f32();
                    let requested = delta.y.as_f32();
                    // A List can coalesce several packets against one painted
                    // scroll position. Their offset difference alone does not
                    // prove overscroll, especially after direction changes or
                    // a zero-delta Ended packet from a trackpad.
                    let at_outward_edge = (requested > 0. && offset.y == px(0.))
                        || (requested < 0. && offset.y == -max);
                    let residual =
                        (requested - (after - before)).clamp(requested.min(0.), requested.max(0.));
                    if at_outward_edge
                        && residual.abs() > 0.01
                        && (!state.physics.suppress_momentum || allow_end_bounce)
                    {
                        let dragging = state.physics.dragging;
                        if !dragging {
                            state.physics.begin(bounds.size.height.as_f32());
                        }
                        state.physics.pull(residual);
                        if !dragging || ended {
                            state.release(!dragging);
                        }
                        changed = true;
                    }
                }
                if changed {
                    state.sampled_at = Some(Instant::now());
                }
                drop(state);
                if changed {
                    cx.notify(view);
                }
                if scrolled && let Some(handler) = &on_scroll {
                    handler(window, cx);
                }
            });
        }
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            self.child.paint(window, cx)
        });
    }
}

#[derive(Default)]
struct Physics {
    position: f32,
    velocity: f32,
    dragging: bool,
    suppress_momentum: bool,
    extent: f32,
    motion: ScrollBounceMotion,
}

impl Physics {
    fn offset(&self) -> f32 {
        if self.dragging {
            let d = self.extent.max(1.);
            let tracking = self.motion.tracking;
            self.position * tracking / (1. + tracking * self.position.abs() / d)
        } else {
            self.position
        }
    }

    fn begin(&mut self, extent: f32) {
        let offset = self.offset();
        // A displaced edge keeps the extent it was stretched under. The
        // rubber-band curve saturates at the extent, so re-reading a viewport
        // that shrank mid-return (rotation, keyboard) could not place the
        // finger where the edge is: it would snap, then need a long pull back.
        if offset == 0. {
            self.extent = extent.max(1.);
        }
        // Invert the rubber-band curve so grabbing a returning edge is continuous.
        let tracking = self.motion.tracking;
        self.position = offset / (tracking * (1. - offset.abs() / self.extent).max(0.01));
        self.velocity = 0.;
        self.dragging = true;
        self.suppress_momentum = false;
    }

    fn pull(&mut self, delta: f32) -> f32 {
        let previous = self.position;
        let next = previous + delta;
        if previous != 0. && previous.signum() != next.signum() {
            self.position = 0.;
            next
        } else {
            self.position = next;
            0.
        }
    }

    fn release(&mut self) {
        self.position = self.offset();
        self.dragging = false;
        if self.position != 0. {
            self.suppress_momentum = true;
        }
    }

    fn step(&mut self, seconds: f32) -> bool {
        if self.dragging || self.position == 0. {
            return false;
        }
        let Some(omega) = self.motion.omega() else {
            self.position = 0.;
            self.velocity = 0.;
            return false;
        };
        let decay = (-omega * seconds).exp();
        let c = self.velocity + omega * self.position;
        self.position = (self.position + c * seconds) * decay;
        self.velocity = (self.velocity - omega * c * seconds) * decay;
        if self.position.abs() < 0.1 && self.velocity.abs() < 1. {
            self.position = 0.;
            self.velocity = 0.;
            false
        } else {
            true
        }
    }
}
