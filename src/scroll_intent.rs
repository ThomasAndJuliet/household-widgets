// SPDX-License-Identifier: GPL-3.0-only
//! Report vertical page-wheel attempts after native child-first dispatch.
//!
//! This wrapper observes one native vertical scrollable and does not report
//! touch, keyboard, programmatic scrolling or general ownership.
use cosmic::iced::advanced::widget::{Operation, Tree, tree};
use cosmic::iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use cosmic::iced::{Event, Length, Rectangle, Size, Vector, keyboard};
use cosmic::{Element, Renderer, Theme};
use std::{cell::Cell, rc::Rc};

/// Build the native scrollable from `content`, preserving its caller-supplied
/// id, scrolling callback and layout. `on_attempt` is sent only for a vertical
/// wheel delivered over its viewport that a child did not capture, or a
/// left-press / pointer motion captured by the native scroller itself.
pub fn vertical<'a, M: 'a>(
    content: Element<'a, M>,
    build: impl FnOnce(Element<'a, M>) -> Element<'a, M>,
    on_attempt: impl Fn() -> M + 'a,
) -> Element<'a, M> {
    let captured = Rc::new(Cell::new(false));
    let observer = Element::new(ChildObserver {
        content,
        captured: captured.clone(),
    });
    Element::new(Vertical {
        content: build(observer),
        captured,
        on_attempt: Box::new(on_attempt),
    })
}
struct ChildObserver<'a, M> {
    content: Element<'a, M>,
    captured: Rc<Cell<bool>>,
}
impl<M> Widget<M, Theme, Renderer> for ChildObserver<'_, M> {
    forward_widget!(M, Theme, Renderer; children, diff, size, size_hint, layout, draw, operate, a11y_nodes, mouse_interaction, overlay, drag_destinations);
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let before = shell.is_event_captured();
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if !before && shell.is_event_captured() {
            self.captured.set(true);
        }
    }
}
struct Vertical<'a, M> {
    content: Element<'a, M>,
    captured: Rc<Cell<bool>>,
    on_attempt: Box<dyn Fn() -> M + 'a>,
}
#[derive(Default)]
struct State {
    modifiers: keyboard::Modifiers,
}
impl<M> Widget<M, Theme, Renderer> for Vertical<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    forward_widget!(M, Theme, Renderer; children, diff, size, size_hint, layout, draw, operate, a11y_nodes, mouse_interaction, overlay, drag_destinations);
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = event {
            state.modifiers = *modifiers;
        }
        let eligible = match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta })
                if cursor.position_over(layout.bounds()).is_some()
                    && cursor.position_over(*viewport).is_some() =>
            {
                let (x, y) = match delta {
                    mouse::ScrollDelta::Lines { x, y } | mouse::ScrollDelta::Pixels { x, y } => {
                        (*x, *y)
                    }
                };
                // Mirrors iced Scrollable's shift-axis choice on macOS and elsewhere.
                let axis = if state.modifiers.shift() && !cfg!(target_os = "macos") {
                    x
                } else {
                    y
                };
                axis != 0.0
            }
            _ => false,
        };
        let already_captured = shell.is_event_captured();
        self.captured.set(false);
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        let scrollbar_attempt = shell.is_event_captured()
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                    | Event::Mouse(mouse::Event::CursorMoved { .. })
            );
        if (eligible || scrollbar_attempt) && !already_captured && !self.captured.get() {
            shell.publish((self.on_attempt)());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic::iced::advanced::{clipboard, renderer::Headless};
    use cosmic::iced::{Font, Pixels, Point};
    use cosmic::widget;

    fn wheel(y: f32) -> Event {
        Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y },
        })
    }
    fn exercise(nested: bool, event: Event, at: Point) -> Vec<()> {
        let renderer = cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
            Font::default(),
            Pixels(14.0),
            Some("tiny-skia"),
        ))
        .unwrap();
        let child: Element<'static, ()> = if nested {
            widget::scrollable(widget::Space::new().width(100).height(500))
                .width(100)
                .height(60)
                .into()
        } else {
            widget::Space::new().width(100).height(500).into()
        };
        let mut view = vertical(
            child,
            |child| widget::scrollable(child).width(100).height(100).into(),
            || (),
        );
        let mut tree = Tree::new(view.as_widget());
        let size = Size::new(100.0, 100.0);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        view.as_widget_mut().update(
            &mut tree,
            &event,
            Layout::new(&node),
            mouse::Cursor::Available(at),
            &renderer,
            &mut clipboard::Null,
            &mut shell,
            &Rectangle::new(Point::ORIGIN, size),
        );
        messages
    }
    #[test]
    fn native_scrollbar_press_is_browsing_but_content_press_is_not() {
        let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        assert_eq!(
            exercise(false, press.clone(), Point::new(99.0, 50.0)).len(),
            1,
            "native scrollbar capture is browsing"
        );
        assert!(exercise(false, press, Point::new(30.0, 30.0)).is_empty());
    }
    #[test]
    fn wheel_attempt_at_boundary_and_nested_capture() {
        let inside = Point::new(30.0, 30.0);
        assert_eq!(
            exercise(false, wheel(1.0), inside).len(),
            1,
            "boundary wheel is browsing"
        );
        assert_eq!(
            exercise(false, wheel(-1.0), inside).len(),
            1,
            "moving page wheel is browsing"
        );
        assert!(
            exercise(true, wheel(-1.0), inside).is_empty(),
            "nested scroller consumed wheel"
        );
        assert!(
            exercise(false, wheel(-1.0), Point::new(120.0, 30.0)).is_empty(),
            "outside viewport"
        );
        assert!(
            exercise(
                false,
                Event::Window(cosmic::iced::window::Event::RedrawRequested(
                    std::time::Instant::now()
                )),
                inside
            )
            .is_empty(),
            "redraw is not browsing"
        );
    }
}
