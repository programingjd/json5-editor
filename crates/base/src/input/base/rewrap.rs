//! Cancellable, cooperative layout updates for large inputs.
use super::kind::InputModeKind;
use super::state::InputBaseState;
use super::{DisplayPoint, WrappingIndent};
use gpui::{AppContext as _, Context, Pixels, Window, point, px};
use std::time::Duration;

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
let layoutChannel;
const layoutContinuations = [];
export function yieldForLayout() {
    if (!layoutChannel) {
        layoutChannel = new MessageChannel();
        layoutChannel.port1.onmessage = () => layoutContinuations.shift()?.();
    }
    return new Promise(resolve => {
        layoutContinuations.push(resolve);
        layoutChannel.port2.postMessage(null);
    });
}
"#)]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = yieldForLayout)]
    fn yield_for_layout() -> js_sys::Promise;
}

async fn yield_layout(_executor: &gpui::BackgroundExecutor) {
    #[cfg(target_family = "wasm")]
    {
        // Posted messages yield to input/painting without the minimum delay
        // imposed by nested setTimeout calls in a long-running resize job.
        let _ = wasm_bindgen_futures::JsFuture::from(yield_for_layout()).await;
    }
    #[cfg(not(target_family = "wasm"))]
    _executor.timer(Duration::from_millis(1)).await;
}

impl<M: InputModeKind> InputBaseState<M> {
    /// Whether a newer wrapping layout is being prepared asynchronously.
    pub fn is_rewrapping(&self) -> bool {
        self.pending_rewrap.is_some()
    }

    pub(super) fn request_rewrap(
        &mut self,
        width: Option<Pixels>,
        indent: WrappingIndent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let request = (width, indent);
        if self.pending_rewrap == Some(request) {
            return;
        }
        // Compare against the installed wrapper, not LastLayout: a paint can
        // already record the requested width while the old wrapping is visible.
        let job = self.display_map.begin_rewrap(width, indent);
        if job.is_none() && self.pending_rewrap.is_none() {
            return;
        }
        self.rewrap_epoch = self.rewrap_epoch.wrapping_add(1);
        self.rewrap_task = None;
        let was_busy = self.pending_rewrap.take().is_some();
        if self.last_layout.is_none() || self.text.len() < 64 * 1024 {
            self.display_map.on_layout_changed(width, cx);
            self.display_map.set_wrapping_indent(indent, cx);
            if was_busy {
                cx.notify();
            }
            return;
        }
        let Some(job) = job else {
            if was_busy {
                cx.notify();
            }
            return;
        };
        if job.can_reuse_all_lines() {
            // The root summary proves that every existing row still fits.
            // Commit only the new width; no debounce, spinner or reflow is needed.
            self.display_map.finish_rewrap(job);
            if was_busy {
                cx.notify();
            }
            return;
        }
        self.pending_rewrap = Some(request);
        let epoch = self.rewrap_epoch;
        cx.notify();
        let executor = cx.background_executor().clone();
        let window_handle = window.window_handle();
        self.rewrap_task = Some(cx.spawn(async move |this, cx| {
            // Notifications raised during prepaint need a fresh frame after
            // that draw unwinds, so the busy state is visible without input.
            let _ = cx.update_window(window_handle, |_, window, _| window.refresh());
            // Coalesce consecutive resize events before starting a replacement layout.
            executor.timer(Duration::from_millis(50)).await;
            let mut job = None;
            let mut restarts = 0;
            loop {
                let complete = this
                    .update(cx, |state, cx| {
                        if state.rewrap_epoch != epoch {
                            return true;
                        }
                        // Text edits, font changes and inline geometry invalidate the snapshot.
                        if job
                            .as_ref()
                            .is_none_or(|job| !state.display_map.rewrap_is_current(job))
                        {
                            restarts += usize::from(job.is_some());
                            job = state.display_map.begin_rewrap(width, indent);
                        }
                        let Some(current) = job.as_mut() else {
                            state.pending_rewrap = None;
                            cx.notify();
                            return true;
                        };
                        if !current.advance(cx) {
                            return false;
                        }
                        let offset = state
                            .deferred_scroll_offset
                            .unwrap_or_else(|| state.scroll_handle.offset());
                        let anchor = state.last_layout.as_ref().map(|layout| {
                            let height = layout.line_height.max(px(1.));
                            let top = (-offset.y / height).max(0.);
                            let row = top.floor() as usize;
                            (
                                state
                                    .display_map
                                    .display_pos_to_buffer_pos(DisplayPoint::new(row, 0)),
                                height,
                                top - row as f32,
                            )
                        });
                        current.report_timing(restarts);
                        let installing = web_time::Instant::now();
                        // Install the complete layout atomically. Source positions and undo stay untouched.
                        state.display_map.finish_rewrap(job.take().unwrap());
                        if let Some((anchor, height, fraction)) = anchor {
                            let row = state.display_map.buffer_pos_to_display_pos(anchor).row;
                            state.deferred_scroll_offset =
                                Some(point(offset.x, -height * (row as f32 + fraction)));
                        }
                        state.mode.update_auto_grow(&state.display_map);
                        tracing::info!(
                            "[JSON5 layout] install/folds: {:.1} ms",
                            installing.elapsed().as_secs_f64() * 1000.
                        );
                        state.pending_rewrap = None;
                        cx.notify();
                        true
                    })
                    .unwrap_or(true);
                if complete {
                    // Invalidate the owning window explicitly after installing
                    // the new wrap map; completion must not wait for scrolling.
                    let _ = cx.update_window(window_handle, |_, window, _| window.refresh());
                    break;
                }
                // A macrotask boundary allows painting and input between bounded wrapping batches.
                yield_layout(&executor).await;
            }
        }));
    }
}
