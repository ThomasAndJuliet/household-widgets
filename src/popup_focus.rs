// SPDX-License-Identifier: GPL-3.0-only

//! Explicit focus participation for content which does not expose a native
//! focusable operation. Already-focusable controls such as text inputs retain
//! their own native contract and do not need this adapter.

use cosmic::iced::advanced::renderer::Renderer as _;
use cosmic::iced::advanced::widget::{Id, Operation, Tree, operation, tree};
use cosmic::iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use cosmic::iced::{Border, Color, Event, Length, Rectangle, Size, Vector, keyboard};
use cosmic::{Element, Renderer, Theme};
use std::rc::Rc;

type TargetBorder = Rc<dyn Fn(&Theme) -> Border>;

/// Give otherwise nonfocusable content an explicit native focus target. The
/// adapter owns `id`; its child should use a different ID if it needs one.
/// Already-focusable editors and controls should use their existing contract.
pub fn focus_target<'a, Message>(
    id: impl Into<Id>,
    content: impl Into<Element<'a, Message>>,
) -> Focusable<'a, Message> {
    Focusable {
        id: id.into(),
        content: content.into(),
        activate: None,
        target_border: None,
    }
}

pub struct Focusable<'a, Message> {
    id: Id,
    content: Element<'a, Message>,
    activate: Option<Message>,
    target_border: Option<TargetBorder>,
}
impl<'a, Message> Focusable<'a, Message> {
    /// Only unhandled Enter/Space activates; arbitrary child events run first.
    pub fn on_activate(mut self, message: Message) -> Self {
        self.activate = Some(message);
        self
    }

    /// Opt into the participating component's own target contour. Generic
    /// focus participation deliberately supplies no independent decoration.
    pub fn target_border(mut self, border: impl Fn(&Theme) -> Border + 'static) -> Self {
        self.target_border = Some(Rc::new(border));
        self
    }
}

#[derive(Default)]
struct State {
    focused: bool,
    keyboard_target: bool,
    redraw: bool,
}

#[cfg(test)]
pub(super) fn keyboard_target_visible(state: &dyn std::any::Any) -> Option<bool> {
    state
        .downcast_ref::<State>()
        .map(|state| state.focused && state.keyboard_target)
}

/// Set target presentation separately from native focus. Hosts invoke this
/// after deliberate keyboard navigation; structural focus restoration does
/// not manufacture a keyboard target, and pointer presses clear it.
pub fn show_keyboard_target(id: Id, visible: bool) -> impl Operation {
    struct Target {
        id: Id,
        visible: bool,
    }
    impl Operation for Target {
        fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
            visit(self);
        }
        fn custom(&mut self, id: Option<&Id>, _: Rectangle, state: &mut dyn std::any::Any) {
            if id == Some(&self.id)
                && let Some(state) = state.downcast_mut::<State>()
            {
                state.redraw |= state.keyboard_target != self.visible;
                state.keyboard_target = self.visible;
            }
        }
    }
    Target { id, visible }
}
impl operation::Focusable for State {
    fn is_focused(&self) -> bool {
        self.focused
    }
    fn focus(&mut self) {
        self.redraw |= !self.focused;
        self.focused = true;
    }
    fn unfocus(&mut self) {
        self.redraw |= self.focused;
        self.focused = false;
    }
}

impl<'a, Message: Clone + 'a> Widget<Message, Theme, Renderer> for Focusable<'a, Message> {
    fn id(&self) -> Option<Id> {
        Some(self.id.clone())
    }
    fn set_id(&mut self, id: Id) {
        self.id = id;
    }
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    forward_widget!(Message, Theme, Renderer; children, diff, size, layout, mouse_interaction, overlay, drag_destinations);
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.focusable(
            Some(&self.id),
            layout.bounds(),
            tree.state.downcast_mut::<State>(),
        );
        operation.custom(
            Some(&self.id),
            layout.bounds(),
            tree.state.downcast_mut::<State>(),
        );
        operation.traverse(&mut |operation| {
            self.content
                .as_widget_mut()
                .operate(&mut tree.children[0], layout, renderer, operation)
        });
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
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
        let state = tree.state.downcast_mut::<State>();
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        ) {
            let focused = cursor.is_over(layout.bounds())
                && cursor
                    .position()
                    .is_some_and(|point| viewport.contains(point));
            state.redraw |= focused != state.focused;
            state.focused = focused;
            state.redraw |= state.keyboard_target;
            state.keyboard_target = false;
        }
        if state.redraw {
            state.redraw = false;
            shell.request_redraw();
        }
        if shell.is_event_captured() || !state.focused {
            return;
        }
        let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
            return;
        };
        if !modifiers.is_empty() {
            return;
        }
        if (matches!(key, keyboard::Key::Named(keyboard::key::Named::Enter))
            || matches!(key, keyboard::Key::Character(value) if value.as_str() == " "))
            && let Some(message) = &self.activate
        {
            shell.publish(message.clone());
            shell.capture_event();
        }
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        if tree.state.downcast_ref::<State>().focused
            && tree.state.downcast_ref::<State>().keyboard_target
            && let Some(border) = &self.target_border
        {
            renderer.with_layer(*viewport, |renderer| {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: layout.bounds(),
                        border: border(theme),
                        ..Default::default()
                    },
                    Color::TRANSPARENT,
                )
            });
        }
    }
    #[cfg(feature = "a11y")]
    fn a11y_nodes(
        &self,
        layout: Layout<'_>,
        tree: &Tree,
        cursor: mouse::Cursor,
    ) -> iced_accessibility::A11yTree {
        self.content
            .as_widget()
            .a11y_nodes(layout, &tree.children[0], cursor)
    }
}
impl<'a, Message: Clone + 'a> From<Focusable<'a, Message>> for Element<'a, Message> {
    fn from(content: Focusable<'a, Message>) -> Self {
        Element::new(content)
    }
}

/// Give an arbitrary native composite a stable named retention boundary.
/// Anonymous children and native controls with composite IDs (for example a
/// scrollable's viewport/scrollbar ID set) remain together when a component
/// moves between physical popup surfaces. Use a name distinct from focus and
/// anchor IDs. This scope adds no geometry, focus behavior, or input policy.
pub fn state_scope<'a, Message: 'a>(
    id: impl Into<Id>,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    Element::new(StateScope {
        id: id.into(),
        content: content.into(),
    })
}

struct StateScope<'a, Message> {
    id: Id,
    content: Element<'a, Message>,
}
struct ScopeState;
impl<'a, Message: 'a> Widget<Message, Theme, Renderer> for StateScope<'a, Message> {
    fn id(&self) -> Option<Id> {
        Some(self.id.clone())
    }
    fn set_id(&mut self, id: Id) {
        self.id = id;
    }
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<ScopeState>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(ScopeState)
    }
    forward_widget!(Message, Theme, Renderer; children, diff, size, layout, draw, update, operate, mouse_interaction, overlay, drag_destinations);
    #[cfg(feature = "a11y")]
    fn a11y_nodes(
        &self,
        layout: Layout<'_>,
        tree: &Tree,
        cursor: mouse::Cursor,
    ) -> iced_accessibility::A11yTree {
        self.content
            .as_widget()
            .a11y_nodes(layout, &tree.children[0], cursor)
    }
}
