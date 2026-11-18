//! Smooth scrolling. iced's scrollable jumps a full step per mouse-wheel notch; this
//! wraps a scrollable, turns wheel notches into a target offset, and the app eases the
//! real offset towards it on every frame (`window::frames` runs only while animating,
//! so idle CPU stays at zero). Touchpad (pixel) scrolling passes through unchanged.

use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::mouse::ScrollDelta;
use iced::widget::scrollable::{AbsoluteOffset, Viewport};
use iced::{Element, Event, Length, Rectangle, Size, Theme, Vector};

/// Pixels per wheel notch (iced/browsers use ~3 lines).
const NOTCH: f32 = 90.0;
/// Exponential approach rate (1/s): ~95% of the distance in ~200 ms.
const RATE: f32 = 15.0;

/// Wraps content and reports mouse-wheel notches instead of letting it scroll.
pub struct WheelCapture<'a, Message> {
    content: Element<'a, Message>,
    on_wheel: Box<dyn Fn(f32) -> Message + 'a>,
}

/// `on_wheel` receives the wheel movement in notches (positive = up/towards older).
pub fn wheel<'a, Message: 'a>(content: impl Into<Element<'a, Message>>, on_wheel: impl Fn(f32) -> Message + 'a) -> Element<'a, Message> {
    Element::new(WheelCapture { content: content.into(), on_wheel: Box::new(on_wheel) })
}

impl<Message> Widget<Message, Theme, iced::Renderer> for WheelCapture<'_, Message> {
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(tree, layout, renderer, operation);
    }

    #[allow(clippy::too_many_arguments)]
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Event::Mouse(mouse::Event::WheelScrolled { delta: ScrollDelta::Lines { y, .. } }) = event
            && *y != 0.0
            && cursor.is_over(layout.bounds())
        {
            shell.publish((self.on_wheel)(*y));
            shell.capture_event();
            return;
        }
        self.content.as_widget_mut().update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &iced::Renderer) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(tree, layout, renderer, viewport, translation)
    }
}

/// Eased scroll position of one scrollable. Offsets are in the scrollable's own anchor
/// space (for the bottom-anchored message list, 0 = newest message).
#[derive(Debug, Default, Clone)]
pub struct Smooth {
    pub current: f32,
    pub target: f32,
    pub max: f32,
    last_frame: Option<Instant>,
    animating: bool,
}

impl Smooth {
    pub fn is_animating(&self) -> bool {
        self.animating
    }

    /// Records the real viewport (user dragged the scrollbar, content changed, …).
    pub fn observe(&mut self, viewport: &Viewport) {
        self.max = (viewport.content_bounds().height - viewport.bounds().height).max(0.0);
        if !self.animating {
            self.current = viewport.absolute_offset().y;
            self.target = self.current;
        }
        self.target = self.target.clamp(0.0, self.max);
    }

    /// Wheel notches → new target. `towards_end` is how a positive (upward) notch moves the offset.
    pub fn wheel(&mut self, notches: f32, upward_increases: bool) {
        let delta = notches * NOTCH * if upward_increases { 1.0 } else { -1.0 };
        self.animate_to(self.target + delta);
    }

    pub fn animate_to(&mut self, target: f32) {
        // Until the first viewport report the content size is unknown; the scrollable
        // clamps the real offset itself and `observe` fixes the target afterwards.
        self.target = if self.max > 0.0 { target.clamp(0.0, self.max) } else { target.max(0.0) };
        if !self.animating {
            self.animating = true;
            self.last_frame = None;
        }
    }

    /// Jumps without animation (e.g. opening a chat).
    pub fn reset(&mut self) {
        self.current = 0.0;
        self.target = 0.0;
        self.animating = false;
    }

    /// Advances one frame; returns the offset to apply, if any.
    pub fn step(&mut self, now: Instant) -> Option<AbsoluteOffset> {
        if !self.animating {
            return None;
        }
        let dt = self.last_frame.map_or(Duration::from_millis(16), |last| now.saturating_duration_since(last)).as_secs_f32().min(0.05);
        self.last_frame = Some(now);
        let distance = self.target - self.current;
        if distance.abs() < 0.5 {
            self.current = self.target;
            self.animating = false;
        } else {
            self.current += distance * (1.0 - (-RATE * dt).exp());
        }
        Some(AbsoluteOffset { x: 0.0, y: self.current })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eases_towards_target_and_stops() {
        let mut smooth = Smooth { max: 1000.0, ..Smooth::default() };
        smooth.wheel(2.0, true);
        assert_eq!(smooth.target, 180.0);
        let start = Instant::now();
        let mut previous = 0.0;
        let mut frames = 0;
        while smooth.is_animating() {
            frames += 1;
            let offset = smooth.step(start + Duration::from_millis(16 * frames)).unwrap();
            assert!(offset.y >= previous, "monotonic");
            previous = offset.y;
            assert!(frames < 120, "settles within 2 s");
        }
        assert_eq!(smooth.current, 180.0);
        assert!(frames > 5, "animates over several frames, got {frames}");
        smooth.wheel(-100.0, true);
        assert_eq!(smooth.target, 0.0, "clamped at the start");
        smooth.reset();
        assert!(!smooth.is_animating());
    }
}
