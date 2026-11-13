// SPDX-License-Identifier: GPL-3.0-only
//! Presentation for navigation between fixed sections. Content tabs and action
//! buttons have different selection and typography rules.

use cosmic::iced::{Background, Color, Vector};
use cosmic::{theme, widget::button};

/// Space reserved on each side for the adaptive keyboard-target outline.
pub const TARGET_OUTSET: f32 = 1.0;
/// Added width or height of a navigation entry's target footprint.
pub const TARGET_FOOTPRINT: f32 = 2.0 * TARGET_OUTSET;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prominence {
    Prominent,
    NonProminent,
}

#[derive(Clone, Copy)]
enum State {
    Rest,
    Hovered,
    Pressed,
    Disabled,
}

/// Native button class. Hover is supplied by the button, never inferred from
/// selection or cached separately. Labels should use `cosmic::font::semibold()`.
fn button_style(prominence: Prominence, current: bool, fallback_focus: bool) -> theme::Button {
    let style = move |state: State, focused: bool, theme: &theme::Theme| {
        let c = theme.cosmic();
        let hovered = matches!(state, State::Hovered | State::Pressed);
        let disabled = matches!(state, State::Disabled);
        let mut wash: Color = c.palette.neutral_5.into();
        wash.a = if hovered { 0.30 } else { 0.20 };
        let (mut fill, on) = crate::muted_colours(theme);
        if disabled {
            fill.a *= 0.5;
            wash.a *= 0.5;
        }
        let prominent = prominence == Prominence::Prominent && current;
        let ink: Color = if prominent {
            on
        } else if current {
            c.accent_text_color().into()
        } else {
            c.on_bg_color().into()
        };
        let mut ink = ink;
        if disabled {
            ink.a *= 0.5;
        }
        let mut result = button::Style {
            shadow_offset: Vector::default(),
            background: if prominent {
                Some(Background::Color(fill))
            } else if current || hovered {
                Some(Background::Color(wash))
            } else {
                None
            },
            overlay: if prominent && hovered {
                Some(Background::Color(wash))
            } else {
                None
            },
            border_radius: c.corner_radii.radius_s.into(),
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
            outline_width: 0.0,
            outline_color: Color::TRANSPARENT,
            icon_color: Some(ink),
            text_color: Some(ink),
        };
        if focused && fallback_focus {
            let border = crate::menu_parts::RowContour::Rounded.target_border(theme);
            result.border_width = border.width;
            result.border_color = border.color;
        }
        result
    };
    theme::Button::Custom {
        active: Box::new(move |f, t| style(State::Rest, f, t)),
        hovered: Box::new(move |f, t| style(State::Hovered, f, t)),
        pressed: Box::new(move |f, t| style(State::Pressed, f, t)),
        disabled: Box::new(move |t| style(State::Disabled, false, t)),
    }
}

/// Standalone native style with a single neutral focus border. Prefer `NavigationButton`
/// for the full adaptive target contour.
pub fn button(prominence: Prominence, current: bool) -> theme::Button {
    button_style(prominence, current, true)
}

/// A fixed-section button. The builder keeps the native button and its target
/// decoration paired, including when the consumer configures activation.
pub struct NavigationButton<'a, M> {
    inner: button::Button<'a, M>,
    prominence: Prominence,
    current: bool,
    press_enabled: bool,
    press_down_enabled: bool,
}

impl<'a, M: Clone + 'a> NavigationButton<'a, M> {
    pub fn new(
        content: impl Into<cosmic::Element<'a, M>>,
        prominence: Prominence,
        current: bool,
    ) -> Self {
        Self {
            inner: button::custom(content).class(button_style(prominence, current, false)),
            prominence,
            current,
            press_enabled: false,
            press_down_enabled: false,
        }
    }
    pub fn id(mut self, id: cosmic::iced::advanced::widget::Id) -> Self {
        self.inner = self.inner.id(id);
        self
    }
    pub fn padding(mut self, padding: impl Into<cosmic::iced::Padding>) -> Self {
        self.inner = self.inner.padding(padding);
        self
    }
    pub fn on_press(mut self, message: M) -> Self {
        self.press_enabled = true;
        self.inner = self.inner.on_press(message);
        self
    }
    pub fn on_press_down(mut self, message: M) -> Self {
        self.press_down_enabled = true;
        self.inner = self.inner.on_press_down(message);
        self
    }
    pub fn on_press_maybe(mut self, message: Option<M>) -> Self {
        self.press_enabled = message.is_some();
        self.inner = self.inner.on_press_maybe(message);
        self
    }
    pub fn width(mut self, width: impl Into<cosmic::iced::Length>) -> Self {
        self.inner = self.inner.width(width);
        self
    }
}

impl<'a, M: Clone + 'a> From<NavigationButton<'a, M>> for cosmic::Element<'a, M> {
    fn from(button: NavigationButton<'a, M>) -> Self {
        let content = cosmic::widget::container(button.inner).padding(TARGET_OUTSET);
        cosmic::Element::new(Target {
            content: content.into(),
            prominence: button.prominence,
            current: button.current,
            enabled: button.press_enabled || button.press_down_enabled,
        })
    }
}

use cosmic::iced::advanced::widget::{Operation, Tree, operation, tree};
use cosmic::iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use cosmic::iced::{Event, Length, Rectangle, Size};
use cosmic::{Element, Renderer, Theme};

// Contrast is resolved from what the native class actually draws. A translucent
// fill depends on its containing surface, so it retains the shared neutral line.
fn displayed_solid(style: &button::Style) -> Option<Color> {
    let Some(Background::Color(fill)) = style.background else {
        return None;
    };
    if fill.a < 1.0 {
        return None;
    }
    Some(
        style
            .overlay
            .map_or(fill, |overlay| crate::kept::flat(overlay, fill)),
    )
}

struct Target<'a, M> {
    content: Element<'a, M>,
    prominence: Prominence,
    current: bool,
    enabled: bool,
}
#[derive(Default)]
struct TargetState {
    focused: bool,
    window_active: bool,
}
#[derive(Default)]
struct Probe(bool);
impl Operation for Probe {
    fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn Operation)) {
        f(self);
    }
    fn focusable(
        &mut self,
        _: Option<&cosmic::iced::advanced::widget::Id>,
        _: Rectangle,
        focus: &mut dyn operation::Focusable,
    ) {
        self.0 |= focus.is_focused();
    }
}
impl<M> Target<'_, M> {
    fn probe(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer) {
        let mut probe = Probe::default();
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, &mut probe);
        tree.state.downcast_mut::<TargetState>().focused = probe.0;
    }
}
impl<M> Widget<M, Theme, Renderer> for Target<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<TargetState>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(TargetState {
            focused: false,
            window_active: true,
        })
    }
    forward_widget!(M, Theme, Renderer; children, diff, size, size_hint, layout, mouse_interaction, overlay, drag_destinations, a11y_nodes);
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        op: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, op);
        self.probe(tree, layout, renderer);
    }
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
        let state = tree.state.downcast_mut::<TargetState>();
        match event {
            Event::Window(cosmic::iced::window::Event::Unfocused) => state.window_active = false,
            Event::Window(cosmic::iced::window::Event::Focused) => state.window_active = true,
            _ => {}
        }
        self.probe(tree, layout, renderer);
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
        use cosmic::iced::advanced::Renderer as _;
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        let state = tree.state.downcast_ref::<TargetState>();
        if !state.focused || !state.window_active {
            return;
        }
        let bounds = layout.bounds();
        let contour = crate::menu_parts::RowContour::Rounded.target_border(theme);
        use cosmic::widget::button::Catalog;
        let class = button_style(self.prominence, self.current, false);
        let button_bounds = Rectangle {
            x: bounds.x + TARGET_OUTSET,
            y: bounds.y + TARGET_OUTSET,
            width: (bounds.width - TARGET_FOOTPRINT).max(0.0),
            height: (bounds.height - TARGET_FOOTPRINT).max(0.0),
        };
        let appearance = if !self.enabled {
            theme.disabled(&class)
        } else if cursor.is_over(button_bounds) {
            theme.hovered(false, false, &class)
        } else {
            theme.active(false, false, &class)
        };
        let double = displayed_solid(&appearance).is_some_and(|fill| {
            household_tones::lighter_on(household_tones::Srgb::new(fill.r, fill.g, fill.b))
        });
        let inner = button_bounds;
        if double {
            let [tl, tr, br, bl]: [f32; 4] = contour.radius.into();
            let outer = cosmic::iced::Border {
                color: contour.color,
                width: 1.0,
                radius: [
                    tl + TARGET_OUTSET,
                    tr + TARGET_OUTSET,
                    br + TARGET_OUTSET,
                    bl + TARGET_OUTSET,
                ]
                .into(),
            };
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: outer,
                    ..Default::default()
                },
                Color::TRANSPARENT,
            );
        }
        renderer.fill_quad(
            renderer::Quad {
                bounds: inner,
                border: cosmic::iced::Border {
                    color: if double { Color::WHITE } else { contour.color },
                    width: 1.0,
                    radius: contour.radius,
                },
                ..Default::default()
            },
            Color::TRANSPARENT,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic::widget::button::Catalog;

    #[test]
    fn native_hover_replaces_the_current_wash_and_keeps_prominent_fill() {
        for theme in [Theme::light(), Theme::dark()] {
            let mut current: Color = theme.cosmic().palette.neutral_5.into();
            current.a = 0.20;
            let mut hover = current;
            hover.a = 0.30;
            let ordinary = button(Prominence::NonProminent, false);
            assert_eq!(theme.active(false, false, &ordinary).background, None);
            assert_eq!(
                theme.hovered(false, false, &ordinary).background,
                Some(Background::Color(hover))
            );
            let selected = button(Prominence::NonProminent, true);
            assert_eq!(
                theme.active(false, false, &selected).background,
                Some(Background::Color(current))
            );
            let hovered = theme.hovered(false, false, &selected);
            assert_eq!(hovered.background, Some(Background::Color(hover)));
            assert_eq!(hovered.overlay, None);
            let prominent = button(Prominence::Prominent, true);
            let rest = theme.active(false, false, &prominent);
            let hovered = theme.hovered(false, false, &prominent);
            assert_eq!(
                rest.background,
                Some(Background::Color(crate::muted_colours(&theme).0))
            );
            assert_eq!(hovered.background, rest.background);
            assert_eq!(hovered.overlay, Some(Background::Color(hover)));
            assert_eq!(hovered.text_color, rest.text_color);
        }
    }
    #[test]
    fn solid_fill_resolution_uses_overlay_and_ignores_translucent_surfaces() {
        let mut style = button::Style {
            background: Some(Background::Color(Color::from_rgb(0.1, 0.1, 0.1))),
            overlay: Some(Background::Color(Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.3,
            })),
            ..Default::default()
        };
        let shown = displayed_solid(&style).unwrap();
        assert!((shown.r - 0.37).abs() < 0.001);
        style.background = Some(Background::Color(Color {
            a: 0.2,
            ..Color::BLACK
        }));
        assert_eq!(displayed_solid(&style), None);
        for theme in [Theme::light(), Theme::dark()] {
            let class = button(Prominence::Prominent, true);
            let disabled = theme.disabled(&class);
            let Some(Background::Color(fill)) = disabled.background else {
                panic!("current fill")
            };
            assert_eq!(fill.a, crate::muted_colours(&theme).0.a * 0.5);
            assert!(displayed_solid(&disabled).is_none());
        }
    }

    #[test]
    fn optional_press_does_not_hide_a_press_down_action() {
        let button =
            NavigationButton::new(cosmic::widget::text("Section"), Prominence::Prominent, true)
                .on_press_down(())
                .on_press_maybe(None);
        assert!(!button.press_enabled && button.press_down_enabled);
        let button =
            NavigationButton::new(cosmic::widget::text("Section"), Prominence::Prominent, true)
                .on_press_maybe(None)
                .on_press_down(());
        assert!(!button.press_enabled && button.press_down_enabled);
    }

    #[test]
    fn native_focus_operation_updates_target_without_relayout() {
        use cosmic::iced::advanced::renderer::Headless;
        use cosmic::iced::{Font, Pixels};
        let renderer = cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
            Font::default(),
            Pixels(14.0),
            Some("tiny-skia"),
        ))
        .expect("headless renderer");
        let mut element: Element<'_, ()> =
            NavigationButton::new(cosmic::widget::text("Section"), Prominence::Prominent, true)
                .on_press(())
                .into();
        let mut tree = Tree::new(element.as_widget());
        let limits = layout::Limits::new(Size::ZERO, Size::new(300.0, 100.0));
        let node = element
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits);
        let before = node.size();
        assert!(!tree.state.downcast_ref::<TargetState>().focused);
        struct Focus;
        impl Operation for Focus {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn Operation)) {
                f(self);
            }
            fn focusable(
                &mut self,
                _: Option<&cosmic::iced::advanced::widget::Id>,
                _: Rectangle,
                state: &mut dyn operation::Focusable,
            ) {
                state.focus();
            }
        }
        element
            .as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut Focus);
        assert!(tree.state.downcast_ref::<TargetState>().focused);
        let after = element
            .as_widget_mut()
            .layout(&mut tree, &renderer, &limits)
            .size();
        assert_eq!(before, after);
    }
}
