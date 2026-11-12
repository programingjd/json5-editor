//! Conservative width intervals reuse the original Unicode wrapping decisions without approximating glyphs.

use gpui::{Pixels, px};

/// All comparisons that chose the current line breaks must retain their truth value.
#[derive(Clone, Copy, Debug)]
pub(super) struct WidthRange {
    /// Inclusive lower bound in logical pixels, rounded inward for floating point subtraction.
    minimum: Pixels,
    /// Exclusive upper bound in logical pixels, also rounded inward.
    maximum: Pixels,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuse_preserves_fit_decisions_and_continuation_indent() {
        let mut widths = WidthRange::all();
        assert!(widths.fits(px(40.), px(50.), px(10.)));
        assert!(!widths.fits(px(55.), px(50.), px(10.)));
        assert!(widths.contains(px(60.)));
        assert!(!widths.contains(px(49.)));
        assert!(!widths.contains(px(65.)));
        assert!(!widths.contains(px(f32::NAN)));
    }

    #[test]
    fn intersected_lines_reuse_only_their_common_interval() {
        let mut first = WidthRange::all();
        first.fits(px(40.), px(50.), px(0.));
        first.fits(px(60.), px(50.), px(0.));
        let mut second = WidthRange::all();
        second.fits(px(45.), px(50.), px(0.));
        second.fits(px(55.), px(50.), px(0.));
        first.intersect(second);
        assert!(first.contains(px(50.)));
        assert!(!first.contains(px(43.)));
        assert!(!first.contains(px(57.)));
        first.intersect(WidthRange::empty());
        assert!(!first.contains(px(50.)));
    }
}

impl WidthRange {
    /// An empty line imposes no width constraint until a glyph measurement narrows it.
    pub(super) fn all() -> Self {
        Self {
            minimum: Pixels::MIN,
            maximum: Pixels::MAX,
        }
    }

    /// Unknown measurement semantics (notably inline elements) must always take the original path.
    pub(super) fn empty() -> Self {
        Self {
            minimum: px(0.),
            maximum: px(0.),
        }
    }

    /// Report the original fit predicate and retain a safe interval for the enclosing viewport.
    /// `indent` is subtracted from its width to obtain `available` on continuation rows.
    pub(super) fn fits(&mut self, measured: Pixels, available: Pixels, indent: Pixels) -> bool {
        let fits = measured <= available;
        let boundary = f32::from(measured + indent);
        if fits {
            self.minimum = self.minimum.max(px(boundary.next_up()));
        } else {
            self.maximum = self.maximum.min(px(boundary.next_down()));
        }
        fits
    }

    /// Combine line/substring constraints; an empty intersection simply disables reuse.
    pub(super) fn intersect(&mut self, other: Self) {
        self.minimum = self.minimum.max(other.minimum);
        self.maximum = self.maximum.min(other.maximum);
    }

    /// NaN and threshold boundaries fail closed, allowing the original measurer to decide.
    pub(super) fn contains(self, width: Pixels) -> bool {
        width >= self.minimum && width < self.maximum
    }
}
