use gpui::{App, ElementId, Window};

use super::{Instant, Interpolate, MotionStatus, Transition, TransitionId};

#[derive(Clone)]
pub struct SequenceStep<T> {
    target: T,
    transition: Transition,
}

impl<T> SequenceStep<T> {
    pub fn new(target: T, transition: Transition) -> Self {
        Self { target, transition }
    }

    pub fn target(&self) -> &T {
        &self.target
    }

    pub fn transition(&self) -> &Transition {
        &self.transition
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SequenceSample<T> {
    value: T,
    step: usize,
    status: MotionStatus,
}

impl<T> SequenceSample<T> {
    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn into_value(self) -> T {
        self.value
    }

    pub fn step(&self) -> usize {
        self.step
    }

    pub fn status(&self) -> MotionStatus {
        self.status
    }

    pub fn is_finished(&self) -> bool {
        self.status == MotionStatus::Finished
    }

    fn is_active(&self) -> bool {
        matches!(self.status, MotionStatus::Delayed | MotionStatus::Running)
    }
}

#[derive(Clone)]
pub struct Sequence<T> {
    id: TransitionId,
    from: T,
    steps: Vec<SequenceStep<T>>,
}

impl<T> Sequence<T>
where
    T: Interpolate + PartialEq + 'static,
{
    pub fn new(id: impl Into<TransitionId>, from: T) -> Self {
        Self {
            id: id.into(),
            from,
            steps: Vec::new(),
        }
    }

    pub fn with_step(mut self, target: T, transition: Transition) -> Self {
        self.steps.push(SequenceStep::new(target, transition));
        self
    }

    pub fn with_steps(mut self, steps: impl IntoIterator<Item = SequenceStep<T>>) -> Self {
        self.steps.extend(steps);
        self
    }

    pub fn from(&self) -> &T {
        &self.from
    }

    pub fn steps(&self) -> &[SequenceStep<T>] {
        &self.steps
    }

    pub fn sample(self, window: &mut Window, cx: &mut App) -> SequenceSample<T> {
        let Some(last) = self.steps.last() else {
            return SequenceSample {
                value: self.from,
                step: 0,
                status: MotionStatus::Idle,
            };
        };
        let last_step = self.steps.len() - 1;

        let id = ElementId::NamedChild(self.id.0.into(), "__sequence".into());
        let now = cx.background_executor().now();
        let state = window.use_keyed_state(id, cx, |_, _| {
            SequenceState::start(0, self.from.clone(), &self.steps, now)
        });

        if cx.reduce_motion() {
            let settled = state.read(cx);
            if settled.step != last_step || settled.from != last.target {
                state.update(cx, |state, _| {
                    *state = SequenceState::start(last_step, last.target.clone(), &self.steps, now);
                });
            }
            return SequenceSample {
                value: last.target.clone(),
                step: last_step,
                status: MotionStatus::Finished,
            };
        }

        let snapshot = state.read(cx);
        let (progress, status) = snapshot.progress(now);
        let retargeted =
            snapshot.step > last_step || self.steps[snapshot.step].target != snapshot.target;
        let handing_over = status == MotionStatus::Finished && snapshot.step < last_step;

        if !retargeted && !handing_over {
            let sample = SequenceSample {
                value: snapshot.value(progress),
                step: snapshot.step,
                status,
            };
            if sample.is_active() {
                window.request_animation_frame();
            }
            return sample;
        }

        // Only a boundary or a restart writes state. A restart begins the new
        // sequence's first step from the value the old one had reached, which
        // is where the eye is. A hand-over starts the next step at the instant
        // the finished one ended, not at `now`, so a frame that lands past
        // the boundary does not start the step late.
        let mut next = if retargeted {
            SequenceState::start(0, snapshot.value(progress), &self.steps, now)
        } else {
            snapshot.hand_over(&self.steps)
        };
        let (value, status) = loop {
            let (progress, status) = next.progress(now);
            if status == MotionStatus::Finished && next.step < last_step {
                next = next.hand_over(&self.steps);
                continue;
            }
            break (next.value(progress), status);
        };
        let sample = SequenceSample {
            value,
            step: next.step,
            status,
        };
        state.update(cx, |state, _| *state = next);
        if sample.is_active() {
            window.request_animation_frame();
        }
        sample
    }
}

#[derive(Clone)]
struct SequenceState<T> {
    step: usize,
    from: T,
    target: T,
    transition: Transition,
    started_at: Instant,
}

impl<T: Interpolate> SequenceState<T> {
    fn start(step: usize, from: T, steps: &[SequenceStep<T>], started_at: Instant) -> Self {
        Self {
            step,
            from,
            target: steps[step].target.clone(),
            transition: steps[step].transition.clone(),
            started_at,
        }
    }

    fn progress(&self, now: Instant) -> (f32, MotionStatus) {
        self.transition.progress(
            now.saturating_duration_since(self.started_at),
            self.transition.duration,
        )
    }

    fn value(&self, progress: f32) -> T {
        self.from
            .interpolate(&self.target, self.transition.sample(progress))
    }

    fn hand_over(&self, steps: &[SequenceStep<T>]) -> Self {
        Self::start(
            self.step + 1,
            self.target.clone(),
            steps,
            self.started_at + self.transition.finishes_after(),
        )
    }
}
