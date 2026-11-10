//! Coffer's keyboard focus ring (`docs/DESIGN.md` → "Interaction states").
//!
//! The ring is a [`size::FOCUS_RING`] line in [`color::MOLTEN`], drawn only
//! while an element has keyboard focus (GPUI's focus-visible: focused, and the
//! last input was a key press). It sits [`size::FOCUS_RING_OFFSET`] outside
//! the element, concentric with its corners, so its radius there is the
//! element's plus the offset. Rows in scrolling lists draw it inset instead,
//! because a scroll container clips anything outside its rows.
//!
//! Coffer's own components draw it with [`FocusRingExt::focus_visible_ring`].
//! Stock `gpui-component` controls draw their own focus style; see
//! [`super::component`].

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AbsoluteLength, Corners, Edges, FocusHandle, InteractiveElement as _, ParentElement, Pixels,
    Styled, Window, div, px,
};

use super::tokens::{color, size};

/// Debug selector of the ring element, for `debug_bounds` in tests.
pub(crate) const RING_SELECTOR: &str = "coffer-focus-ring";

/// Where a focus ring sits relative to the element it marks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FocusRingPlacement {
    /// [`size::FOCUS_RING_OFFSET`] outside the element. For controls, which
    /// must leave the ring room rather than clip it.
    #[default]
    Outside,
    /// On the inside of the element's edge, following its corners. For rows
    /// in scrolling lists, where an outside ring would be clipped.
    Inset,
}

/// The ring as a box drawn around the element.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RingGeometry {
    /// How far the ring's outer edge sits outside the element's outer
    /// (border) edge.
    outset: Pixels,
    /// The ring's line width.
    width: Pixels,
    /// The ring's outer corner radii. Its inner corners are these less
    /// `width`, as for any border.
    radii: Corners<Pixels>,
}

impl RingGeometry {
    /// The ring for an element with `element_radii`, in a window at
    /// `scale_factor`.
    ///
    /// GPUI draws a border a whole number of device pixels wide and snaps the
    /// box's edges to device pixels, rounding halves towards zero. Given the
    /// spec's 1.5px width at 1x, it draws 1px and moves both edges up and
    /// left by half a pixel, so the gap comes out 3px on the top and left and
    /// 2px on the bottom and right. So the width and the gap are snapped here
    /// first, the way GPUI would snap them, and the outset is built from the
    /// snapped values: the gap is then the same on every side. At 2x both are
    /// already whole device pixels and nothing changes.
    fn new(
        placement: FocusRingPlacement,
        element_radii: Corners<Pixels>,
        scale_factor: f32,
    ) -> Self {
        let width = snap_stroke(size::FOCUS_RING, scale_factor);
        let outset = match placement {
            FocusRingPlacement::Outside => {
                snap_offset(size::FOCUS_RING_OFFSET, scale_factor) + width
            }
            FocusRingPlacement::Inset => Pixels::ZERO,
        };
        Self {
            outset,
            width,
            // Growing the box by `outset` keeps every corner concentric with
            // the element's.
            radii: element_radii.map(|radius| *radius + outset),
        }
    }
}

/// `logical` in device pixels, rounded halves towards zero, as GPUI rounds
/// box edges (`round_half_toward_zero` in `gpui`'s `util.rs`).
fn device_pixels(logical: Pixels, scale_factor: f32) -> f32 {
    let device = f32::from(logical) * scale_factor;
    (device.abs() - 0.5).ceil().copysign(device)
}

/// A line width as GPUI draws it: whole device pixels, at least one.
fn snap_stroke(width: Pixels, scale_factor: f32) -> Pixels {
    px(device_pixels(width, scale_factor).max(1.) / scale_factor)
}

/// A distance as whole device pixels.
fn snap_offset(distance: Pixels, scale_factor: f32) -> Pixels {
    px(device_pixels(distance, scale_factor) / scale_factor)
}

/// Draws Coffer's focus ring on an element.
pub trait FocusRingExt: Styled + ParentElement + Sized {
    /// Draws the focus ring while `focus` has keyboard focus; otherwise
    /// leaves the element unchanged. See [`show_focus_ring`](Self::show_focus_ring)
    /// for how it's drawn.
    fn focus_visible_ring(
        self,
        focus: &FocusHandle,
        placement: FocusRingPlacement,
        window: &Window,
    ) -> Self {
        if focus.is_focused(window) && window.last_input_was_keyboard() {
            self.show_focus_ring(placement, window)
        } else {
            self
        }
    }

    /// Draws the focus ring whatever has focus.
    ///
    /// For an owner that tracks keyboard focus itself, such as a list whose
    /// current row is the keyboard target, and for the showcase's state
    /// matrices. A control with its own focus handle uses
    /// [`focus_visible_ring`](Self::focus_visible_ring) instead.
    ///
    /// Call it after the element's border and corner radius are set: the
    /// ring follows both. It's drawn as the element's last child, absolutely
    /// positioned, so it takes no layout space and doesn't catch the pointer.
    /// An [`Outside`](FocusRingPlacement::Outside) ring needs
    /// [`size::FOCUS_RING_OFFSET`] + [`size::FOCUS_RING`] of room: an ancestor
    /// that clips its content cuts it off, which is what
    /// [`Inset`](FocusRingPlacement::Inset) is for.
    fn show_focus_ring(mut self, placement: FocusRingPlacement, window: &Window) -> Self {
        let rem_size = window.rem_size();
        let pixels = |length: Option<AbsoluteLength>| {
            length.map_or(Pixels::ZERO, |length| length.to_pixels(rem_size))
        };
        let style = self.style();
        let border = Edges {
            top: pixels(style.border_widths.top),
            right: pixels(style.border_widths.right),
            bottom: pixels(style.border_widths.bottom),
            left: pixels(style.border_widths.left),
        };
        let radii = Corners {
            top_left: pixels(style.corner_radii.top_left),
            top_right: pixels(style.corner_radii.top_right),
            bottom_right: pixels(style.corner_radii.bottom_right),
            bottom_left: pixels(style.corner_radii.bottom_left),
        };
        let ring = RingGeometry::new(placement, radii, window.scale_factor());

        // Absolute children are placed against the padding box, inside the
        // border, so each side also steps over the element's border.
        self.child(
            div()
                .when(cfg!(test), |this| {
                    this.debug_selector(|| RING_SELECTOR.into())
                })
                .absolute()
                .top(-(ring.outset + border.top))
                .right(-(ring.outset + border.right))
                .bottom(-(ring.outset + border.bottom))
                .left(-(ring.outset + border.left))
                .border(ring.width)
                .border_color(color::MOLTEN)
                .rounded_tl(ring.radii.top_left)
                .rounded_tr(ring.radii.top_right)
                .rounded_br(ring.radii.bottom_right)
                .rounded_bl(ring.radii.bottom_left),
        )
    }
}

impl<T: Styled + ParentElement + Sized> FocusRingExt for T {}

#[cfg(test)]
mod tests {
    use gpui_kit::{
        Bounds, Context, Entity, IntoElement, Modifiers, Render, TestAppContext, VisualTestContext,
        point,
    };

    use super::super::tokens::radius;
    use super::*;

    const CONTROL: &str = "control";

    /// Distinct corners, so a test catches a corner that's swapped or
    /// dropped.
    fn element_radii() -> Corners<Pixels> {
        Corners {
            top_left: radius::SM,
            top_right: radius::MD,
            bottom_right: radius::LG,
            bottom_left: Pixels::ZERO,
        }
    }

    #[test]
    fn outside_ring_matches_the_spec_at_2x() {
        let ring = RingGeometry::new(FocusRingPlacement::Outside, element_radii(), 2.);

        assert_eq!(ring.width, px(1.5));
        // The gap between the element and the ring's inner edge.
        assert_eq!(ring.outset - ring.width, size::FOCUS_RING_OFFSET);
        assert_eq!(size::FOCUS_RING_OFFSET, px(2.));
        // Where the ring meets the gap, its radius is the element's + 2.
        let inner = ring.radii.map(|radius| *radius - ring.width);
        assert_eq!(inner, element_radii().map(|radius| *radius + px(2.)));
    }

    #[test]
    fn outside_ring_lands_on_whole_pixels_at_1x() {
        let ring = RingGeometry::new(FocusRingPlacement::Outside, element_radii(), 1.);

        // GPUI draws 1.5px as 1px at 1x (as CSS snaps a border width); the
        // gap stays 2px, and the outset is whole so the gap is even.
        assert_eq!(ring.width, px(1.));
        assert_eq!(ring.outset, px(3.));
        let inner = ring.radii.map(|radius| *radius - ring.width);
        assert_eq!(inner, element_radii().map(|radius| *radius + px(2.)));
    }

    #[test]
    fn ring_snaps_to_device_pixels_at_fractional_scales() {
        for scale_factor in [1.25, 1.5, 1.75] {
            let ring =
                RingGeometry::new(FocusRingPlacement::Outside, element_radii(), scale_factor);
            for length in [ring.width, ring.outset] {
                let device = f32::from(length) * scale_factor;
                assert!(
                    (device - device.round()).abs() < 1e-4,
                    "{length:?} at {scale_factor}x is {device} device pixels"
                );
            }
            assert!(ring.width >= px(1. / scale_factor));
        }
    }

    #[test]
    fn inset_ring_lies_on_the_inside_of_the_edge() {
        let ring = RingGeometry::new(FocusRingPlacement::Inset, element_radii(), 2.);

        assert_eq!(ring.width, size::FOCUS_RING);
        assert_eq!(ring.outset, Pixels::ZERO);
        assert_eq!(ring.radii, element_radii());
    }

    /// A bordered, rounded control that draws the ring.
    struct Harness {
        focus: FocusHandle,
        placement: FocusRingPlacement,
    }

    impl Render for Harness {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().p(size::CONTROL).child(
                div()
                    .id(CONTROL)
                    .debug_selector(|| CONTROL.into())
                    .track_focus(&self.focus)
                    .size(size::CONTROL)
                    .border_1()
                    .rounded(radius::MD)
                    .focus_visible_ring(&self.focus, self.placement, window),
            )
        }
    }

    fn harness(
        cx: &mut TestAppContext,
        placement: FocusRingPlacement,
    ) -> (Entity<Harness>, &mut VisualTestContext) {
        let (view, cx) = cx.add_window_view(move |_, cx| Harness {
            focus: cx.focus_handle(),
            placement,
        });
        draw(cx);
        (view, cx)
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn focus(view: &Entity<Harness>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let focus = view.read(cx).focus.clone();
            window.focus(&focus, cx);
            assert!(focus.is_focused(window));
        });
    }

    fn move_pointer(cx: &mut VisualTestContext) {
        cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::none());
        draw(cx);
    }

    fn press_key(cx: &mut VisualTestContext) {
        cx.simulate_keystrokes("a");
        draw(cx);
    }

    fn dilate(bounds: Bounds<Pixels>, amount: Pixels) -> Bounds<Pixels> {
        Bounds::from_corners(
            point(bounds.left() - amount, bounds.top() - amount),
            point(bounds.right() + amount, bounds.bottom() + amount),
        )
    }

    #[gpui_kit::test]
    fn ring_shows_only_for_keyboard_focus(cx: &mut TestAppContext) {
        let (view, cx) = harness(cx, FocusRingPlacement::Outside);
        press_key(cx);
        assert!(
            cx.debug_bounds(RING_SELECTOR).is_none(),
            "unfocused: no ring"
        );

        focus(&view, cx);
        move_pointer(cx);
        assert!(
            cx.debug_bounds(RING_SELECTOR).is_none(),
            "pointer input: no ring"
        );

        press_key(cx);
        let ring = cx
            .debug_bounds(RING_SELECTOR)
            .expect("keyboard focus draws the ring");
        let control = cx.debug_bounds(CONTROL).expect("control is drawn");
        assert_eq!(
            ring,
            dilate(control, size::FOCUS_RING_OFFSET + size::FOCUS_RING)
        );

        move_pointer(cx);
        assert!(
            cx.debug_bounds(RING_SELECTOR).is_none(),
            "pointer again: no ring"
        );
    }

    #[gpui_kit::test]
    fn outside_ring_keeps_an_even_gap_at_1x(cx: &mut TestAppContext) {
        let (view, cx) = harness(cx, FocusRingPlacement::Outside);
        cx.simulate_scale_factor_change(1.);
        focus(&view, cx);
        press_key(cx);

        let ring = cx
            .debug_bounds(RING_SELECTOR)
            .expect("keyboard focus draws the ring");
        let control = cx.debug_bounds(CONTROL).expect("control is drawn");
        // A 1px ring 2px out on every side: whole pixels, so GPUI's edge
        // snapping can't shift it.
        assert_eq!(ring, dilate(control, px(3.)));
    }

    #[gpui_kit::test]
    fn inset_ring_covers_the_element(cx: &mut TestAppContext) {
        let (view, cx) = harness(cx, FocusRingPlacement::Inset);
        focus(&view, cx);
        press_key(cx);

        let ring = cx
            .debug_bounds(RING_SELECTOR)
            .expect("keyboard focus draws the ring");
        assert_eq!(ring, cx.debug_bounds(CONTROL).expect("control is drawn"));
    }
}
