#[cfg(not(target_family = "wasm"))]
use std::time::Instant;
use std::{rc::Rc, time::Duration};
#[cfg(target_family = "wasm")]
use web_time::Instant;

use gpui::{
    App, Bounds, ElementId, Pixels, SharedString, Size, SpringConfig, SpringState, SpringTarget,
    Window,
};

use crate::animation::{Lerp, ease_out_cubic};

mod easing;
mod keyframes;
mod presence;
mod reveal;
mod sequence;
mod stagger;
mod timing;

pub use easing::{Easing, EasingError, LinearStop, StepPosition};
pub use keyframes::{Discrete, DiscreteError, Keyframe, KeyframeError, Keyframes};
pub use presence::{Presence, PresencePhase, PresenceSample};
pub use reveal::MotionReveal;
pub use sequence::{Sequence, SequenceSample, SequenceStep};
pub use stagger::{Stagger, StaggerOrigin};
pub use timing::{
    IterationCount, MotionPhase, PlaybackDirection, SignedDuration, Timing, TimingSample,
};

const DEFAULT_SPRING_EPSILON: f32 = 0.001;

pub trait Interpolate: Clone {
    fn interpolate(&self, target: &Self, progress: f32) -> Self;
}

impl<T: Lerp> Interpolate for T {
    fn interpolate(&self, target: &Self, progress: f32) -> Self {
        self.lerp(target, progress)
    }
}

impl Interpolate for Size<Pixels> {
    fn interpolate(&self, target: &Self, progress: f32) -> Self {
        Size::new(
            self.width.lerp(&target.width, progress),
            self.height.lerp(&target.height, progress),
        )
    }
}

impl Interpolate for Bounds<Pixels> {
    fn interpolate(&self, target: &Self, progress: f32) -> Self {
        Bounds::new(
            self.origin.lerp(&target.origin, progress),
            self.size.interpolate(&target.size, progress),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionTransform {
    pub translation: gpui::Point<Pixels>,
    pub scale: gpui::Point<f32>,
    pub rotation_radians: f32,
    pub opacity: f32,
}

impl MotionTransform {
    pub fn identity() -> Self {
        Self {
            translation: gpui::point(gpui::px(0.0), gpui::px(0.0)),
            scale: gpui::point(1.0, 1.0),
            rotation_radians: 0.0,
            opacity: 1.0,
        }
    }
}

impl Default for MotionTransform {
    fn default() -> Self {
        Self::identity()
    }
}

impl Interpolate for MotionTransform {
    fn interpolate(&self, target: &Self, progress: f32) -> Self {
        Self {
            translation: self.translation.lerp(&target.translation, progress),
            scale: gpui::point(
                self.scale.x.lerp(&target.scale.x, progress),
                self.scale.y.lerp(&target.scale.y, progress),
            ),
            rotation_radians: self
                .rotation_radians
                .lerp(&target.rotation_radians, progress),
            opacity: self.opacity.lerp(&target.opacity, progress),
        }
    }
}

#[derive(Clone)]
pub struct Transition {
    duration: Duration,
    delay: SignedDuration,
    easing: Easing,
}

impl Transition {
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            delay: SignedDuration::ZERO,
            easing: Easing::Custom(Rc::new(ease_out_cubic)),
        }
    }

    pub fn delay(mut self, delay: impl Into<SignedDuration>) -> Self {
        self.delay = delay.into();
        self
    }

    pub fn ease(mut self, easing: impl Fn(f32) -> f32 + 'static) -> Self {
        self.easing = Easing::Custom(Rc::new(easing));
        self
    }

    pub fn easing(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    fn sample(&self, progress: f32) -> f32 {
        self.easing.sample(progress)
    }

    fn progress(&self, elapsed: Duration, duration: Duration) -> (f32, MotionStatus) {
        let Some(active_elapsed) = self.delay.active_elapsed(elapsed) else {
            return (0.0, MotionStatus::Delayed);
        };
        if duration.is_zero() || active_elapsed >= duration {
            return (1.0, MotionStatus::Finished);
        }
        (
            active_elapsed.as_secs_f32() / duration.as_secs_f32(),
            MotionStatus::Running,
        )
    }

    fn finishes_after(&self) -> Duration {
        match self.delay {
            SignedDuration::Positive(delay) => delay.saturating_add(self.duration),
            SignedDuration::Negative(delay) => self.duration.saturating_sub(delay),
        }
    }
}

impl From<Duration> for SignedDuration {
    fn from(duration: Duration) -> Self {
        Self::positive(duration)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TransitionId(ElementId);

impl From<ElementId> for TransitionId {
    fn from(id: ElementId) -> Self {
        Self(id)
    }
}

impl From<&'static str> for TransitionId {
    fn from(id: &'static str) -> Self {
        Self(id.into())
    }
}

impl From<String> for TransitionId {
    fn from(id: String) -> Self {
        Self(id.into())
    }
}

impl From<SharedString> for TransitionId {
    fn from(id: SharedString) -> Self {
        Self(id.into())
    }
}

impl From<usize> for TransitionId {
    fn from(id: usize) -> Self {
        Self(id.into())
    }
}

impl From<i32> for TransitionId {
    fn from(id: i32) -> Self {
        Self(id.into())
    }
}

impl From<TransitionId> for ElementId {
    fn from(id: TransitionId) -> Self {
        ElementId::NamedChild(id.0.into(), "__base-transition-state".into())
    }
}

impl<I, C> From<(I, C)> for TransitionId
where
    I: Into<ElementId>,
    C: Into<SharedString>,
{
    fn from((id, channel): (I, C)) -> Self {
        Self(ElementId::NamedChild(id.into().into(), channel.into()))
    }
}

#[derive(Clone)]
struct ValueTransition<T> {
    from: T,
    target: T,
    started_at: Instant,
    reversing_factor: f32,
    duration: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionStatus {
    Idle,
    Delayed,
    Running,
    Finished,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionValue<T> {
    pub value: T,
    pub status: MotionStatus,
}

pub fn transition<T>(
    id: impl Into<TransitionId>,
    target: T,
    policy: Transition,
    window: &mut Window,
    cx: &mut App,
) -> T
where
    T: Interpolate + PartialEq + 'static,
{
    transition_with_status(id, target, policy, window, cx).value
}

pub fn transition_with_status<T>(
    id: impl Into<TransitionId>,
    target: T,
    policy: Transition,
    window: &mut Window,
    cx: &mut App,
) -> MotionValue<T>
where
    T: Interpolate + PartialEq + 'static,
{
    let id: ElementId = id.into().into();
    let now = cx.background_executor().now();
    let state = window.use_keyed_state(id, cx, |_, _| ValueTransition {
        from: target.clone(),
        target: target.clone(),
        started_at: now,
        reversing_factor: 1.0,
        duration: policy.duration,
    });

    let snapshot = state.read(cx).clone();

    if cx.reduce_motion() || policy.duration.is_zero() {
        if snapshot.from != target || snapshot.target != target {
            state.update(cx, |state, _| {
                state.from = target.clone();
                state.target = target.clone();
                state.started_at = now;
                state.reversing_factor = 1.0;
                state.duration = policy.duration;
            });
        }
        return MotionValue {
            value: target,
            status: MotionStatus::Finished,
        };
    }

    let elapsed = now.saturating_duration_since(snapshot.started_at);
    let (progress, status) = policy.progress(elapsed, snapshot.duration);
    let sampled = snapshot
        .from
        .interpolate(&snapshot.target, policy.sample(progress));

    let (value, status) = if snapshot.target != target {
        let reversing = target == snapshot.from;
        let reversing_factor = if reversing {
            (policy.sample(progress) * snapshot.reversing_factor
                + (1.0 - snapshot.reversing_factor))
                .clamp(0.0, 1.0)
        } else {
            1.0
        };
        let duration = policy.duration.mul_f32(reversing_factor);
        state.update(cx, |state, _| {
            state.from = sampled.clone();
            state.target = target.clone();
            state.started_at = now;
            state.reversing_factor = reversing_factor;
            state.duration = duration;
        });
        let (initial_progress, initial_status) = policy.progress(Duration::ZERO, duration);
        (
            sampled.interpolate(&target, policy.sample(initial_progress)),
            initial_status,
        )
    } else {
        (
            sampled,
            if snapshot.from == snapshot.target {
                MotionStatus::Idle
            } else {
                status
            },
        )
    };
    if matches!(status, MotionStatus::Delayed | MotionStatus::Running) {
        window.request_animation_frame();
    }
    MotionValue { value, status }
}

#[derive(Clone, Copy)]
struct KeyframePlayback {
    started_at: Instant,
}

pub fn animate_keyframes<T>(
    id: impl Into<TransitionId>,
    keyframes: &Keyframes<T>,
    timing: Timing,
    window: &mut Window,
    cx: &mut App,
) -> MotionValue<T>
where
    T: Interpolate + 'static,
{
    let id: TransitionId = id.into();
    let id = ElementId::NamedChild(ElementId::from(id).into(), "__keyframes".into());
    let now = cx.background_executor().now();
    let state = window.use_keyed_state(id, cx, |_, _| KeyframePlayback { started_at: now });
    let started_at = state.read(cx).started_at;

    if cx.reduce_motion() {
        return MotionValue {
            value: keyframes.sample(1.0),
            status: MotionStatus::Finished,
        };
    }

    let sample = timing.sample(now.saturating_duration_since(started_at));
    let status = match sample.phase {
        MotionPhase::Before => MotionStatus::Delayed,
        MotionPhase::Active => MotionStatus::Running,
        MotionPhase::After => MotionStatus::Finished,
    };
    if matches!(status, MotionStatus::Delayed | MotionStatus::Running) {
        window.request_animation_frame();
    }
    MotionValue {
        value: keyframes.sample(sample.directed_progress),
        status,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Spring {
    response: Duration,
    damping: f32,
    epsilon: f32,
    travel: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpringError {
    InvalidDamping,
    InvalidEpsilon,
}

impl std::fmt::Display for SpringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDamping => f.write_str("spring damping must be finite and non-negative"),
            Self::InvalidEpsilon => {
                f.write_str("spring epsilon must be finite and greater than zero")
            }
        }
    }
}

impl std::error::Error for SpringError {}

impl Spring {
    pub const fn new(response: Duration) -> Self {
        Self {
            response,
            damping: 1.0,
            epsilon: DEFAULT_SPRING_EPSILON,
            travel: true,
        }
    }

    pub const fn with_damping(self, ratio: f32) -> Self {
        match self.try_with_damping(ratio) {
            Ok(spring) => spring,
            Err(_) => panic!("spring damping must be finite and non-negative"),
        }
    }

    pub const fn try_with_damping(mut self, ratio: f32) -> Result<Self, SpringError> {
        if !ratio.is_finite() || ratio < 0.0 {
            return Err(SpringError::InvalidDamping);
        }
        self.damping = ratio;
        Ok(self)
    }

    pub const fn with_travel(mut self, travel: bool) -> Self {
        self.travel = travel;
        self
    }

    pub const fn with_epsilon(self, epsilon: f32) -> Self {
        match self.try_with_epsilon(epsilon) {
            Ok(spring) => spring,
            Err(_) => panic!("spring epsilon must be finite and greater than zero"),
        }
    }

    pub const fn try_with_epsilon(mut self, epsilon: f32) -> Result<Self, SpringError> {
        if !epsilon.is_finite() || epsilon <= 0.0 {
            return Err(SpringError::InvalidEpsilon);
        }
        self.epsilon = epsilon;
        Ok(self)
    }

    pub const fn epsilon(self) -> f32 {
        self.epsilon
    }

    fn config(&self) -> SpringConfig {
        let frequency = std::f32::consts::TAU / self.response.as_secs_f32();
        SpringConfig::new(frequency * frequency, 2.0 * self.damping * frequency, 1.0)
    }
}

#[derive(Clone, Copy)]
struct SpringTransition {
    state: SpringState,
    target: f32,
    updated_at: Instant,
}

pub fn spring<T>(
    id: impl Into<TransitionId>,
    target: T,
    policy: Spring,
    window: &mut Window,
    cx: &mut App,
) -> T::Output
where
    T: SpringTarget,
{
    let id: ElementId = id.into().into();
    let now = cx.background_executor().now();
    let target_position = target.target();
    let state = window.use_keyed_state(id, cx, |_, _| SpringTransition {
        state: SpringState {
            position: target_position,
            velocity: 0.0,
        },
        target: target_position,
        updated_at: now,
    });

    let snapshot = *state.read(cx);
    let at_rest_on_target =
        snapshot.state.position == target_position && snapshot.state.velocity == 0.0;

    // The overwhelmingly common case: a spring nothing is currently moving. It
    // has no state to advance and no frame to ask for, so it never builds a
    // config or steps one — a settled spring costs a read and two comparisons.
    // Every branch below would return this same value and write nothing.
    //
    // Resting writes nothing, so `updated_at` goes stale for as long as the rest
    // lasts. The next retarget then steps a zero displacement at zero velocity
    // over that whole gap, which any elapsed time leaves where it is, so the
    // stale clock cannot move the value — it only has to not produce a NaN, and
    // every term the propagator scales is finite.
    if at_rest_on_target {
        return target.resolve(target_position);
    }

    let settle = |state: &mut SpringTransition| {
        state.state = SpringState {
            position: target_position,
            velocity: 0.0,
        };
        state.target = target_position;
        state.updated_at = now;
    };

    if cx.reduce_motion() || !policy.travel || policy.response.is_zero() {
        state.update(cx, |state, _| settle(state));
        return target.resolve(target_position);
    }

    // Advance over the frame that just elapsed, which the previous target
    // governed, before adopting the new one for the frame to come.
    let elapsed = now
        .saturating_duration_since(snapshot.updated_at)
        .as_secs_f32();
    let config = policy.config();
    let stepped = config.step(snapshot.state, snapshot.target, elapsed);

    if config.is_settled(stepped, target_position, policy.epsilon) {
        state.update(cx, |state, _| settle(state));
        return target.resolve(target_position);
    }

    state.update(cx, |state, _| {
        state.state = stepped;
        state.target = target_position;
        state.updated_at = now;
    });
    window.request_animation_frame();
    target.resolve(stepped.position)
}
