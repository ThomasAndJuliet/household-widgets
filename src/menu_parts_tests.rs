// SPDX-License-Identifier: GPL-3.0-only
use crate::menu_parts::*;
use cosmic::iced::advanced::{Layout, Shell, layout, mouse, renderer::Headless, widget::Tree};
use cosmic::iced::widget;
use cosmic::iced::{Event, Font, Pixels, Point, Rectangle, Size};
use cosmic::{Element, Renderer};

fn renderer() -> Renderer {
    cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
        Font::default(),
        Pixels(14.0),
        Some("tiny-skia"),
    ))
    .expect("headless renderer")
}
fn space(width: f32) -> Element<'static, u8> {
    widget::Space::new().width(width).height(20.0).into()
}
fn layout_of(element: &mut Element<'_, u8>, renderer: &Renderer) -> layout::Node {
    let mut tree = Tree::new(element.as_widget());
    element.as_widget_mut().layout(
        &mut tree,
        renderer,
        &layout::Limits::new(Size::ZERO, Size::new(240.0, 200.0)),
    )
}

#[test]
fn blank_leading_slots_keep_content_aligned_and_trailing_shares_cell() {
    let renderer = renderer();
    let slots = RowSlots {
        mark_width: Some(16.0),
        icon_width: Some(20.0),
        gap: 4.0,
    };
    let mut marked = row_content(
        slots,
        Some(space(8.0)),
        Some(space(12.0)),
        space(40.0),
        Some(space(24.0)),
    );
    let mut blank = row_content(slots, None, None, space(70.0), Some(space(24.0)));
    let marked = layout_of(&mut marked, &renderer);
    let blank = layout_of(&mut blank, &renderer);
    let a = marked.children();
    let b = blank.children();
    assert_eq!(a.len(), 3);
    assert_eq!(a[0].size().width, 16.0);
    assert_eq!(b[1].size().width, 20.0);
    assert_eq!(a[2].bounds().x, b[2].bounds().x);
    assert_eq!(a[2].children().len(), 2);
    assert_eq!(a[2].children()[1].bounds().x, b[2].children()[1].bounds().x);
}

fn click(element: &mut Element<'_, u8>, renderer: &Renderer) -> Vec<u8> {
    let mut tree = Tree::new(element.as_widget());
    let node = element.as_widget_mut().layout(
        &mut tree,
        renderer,
        &layout::Limits::new(Size::ZERO, Size::new(240.0, 200.0)),
    );
    let mut messages = Vec::new();
    for event in [
        mouse::Event::ButtonPressed(mouse::Button::Left),
        mouse::Event::ButtonReleased(mouse::Button::Left),
    ] {
        element.as_widget_mut().update(
            &mut tree,
            &Event::Mouse(event),
            Layout::new(&node),
            mouse::Cursor::Available(Point::new(8.0, 8.0)),
            renderer,
            &mut cosmic::iced::advanced::clipboard::Null,
            &mut Shell::new(&mut messages),
            &Rectangle {
                x: 0.0,
                y: 0.0,
                width: 240.0,
                height: 200.0,
            },
        );
    }
    messages
}

#[test]
fn disabled_row_does_not_activate_and_enabled_row_does() {
    let renderer = renderer();
    let mut disabled = action_row(
        space(40.0),
        TooltipChoice::Tooltip {
            content: space(30.0),
            position: widget::tooltip::Position::FollowCursor,
        },
        None,
    );
    let mut enabled = action_row(space(40.0), TooltipChoice::NoTooltip, Some(7));
    assert!(click(&mut disabled, &renderer).is_empty());
    assert_eq!(click(&mut enabled, &renderer), vec![7]);
}

#[test]
fn expansion_is_app_owned_and_does_not_change_header_message() {
    let renderer = renderer();
    let header = || action_row(space(40.0), TooltipChoice::NoTooltip, Some(3));
    let mut closed = expandable(header(), false, vec![space(30.0)]);
    let mut open = expandable(header(), true, vec![space(30.0)]);
    assert_eq!(layout_of(&mut closed, &renderer).children().len(), 1);
    assert_eq!(layout_of(&mut open, &renderer).children().len(), 2);
    assert_eq!(click(&mut closed, &renderer), vec![3]);
    assert_eq!(click(&mut open, &renderer), vec![3]);
}

#[test]
fn label_owns_the_flexible_remainder_and_trailing_stays_at_the_right_edge() {
    use cosmic::iced::advanced::widget::{Id, Operation};
    #[derive(Default)]
    struct TextBounds(Option<Rectangle>);
    impl Operation for TextBounds {
        fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
            visit(self);
        }
        fn text(&mut self, _: Option<&Id>, bounds: Rectangle, _: &str) {
            self.0 = Some(bounds);
        }
    }
    fn label(text: &str) -> Element<'static, u8> {
        widget::text(text.to_owned())
            .size(14)
            .wrapping(cosmic::iced::advanced::text::Wrapping::WordOrGlyph)
            .into()
    }
    let renderer = renderer();
    let natural = crate::layout::child(|_| label("Permission mode"))
        .query(crate::layout::Level::Preferred, &renderer)
        .preferred;
    let slots = RowSlots::default();
    let trailing_width = 16.0;
    let preferred = (natural.width + slots.gap + trailing_width).ceil();
    for already_fills in [false, true] {
        for (text, width, wraps) in [
            ("Permission mode", preferred, false),
            (
                "OneLongUnbrokenPermissionModeLabelThatMustWrap",
                preferred,
                true,
            ),
        ] {
            let label = label(text);
            let label = if already_fills {
                widget::container(label)
                    .width(cosmic::iced::Length::Fill)
                    .into()
            } else {
                label
            };
            let mut element = row_content(slots, None, None, label, Some(space(trailing_width)));
            let mut tree = Tree::new(element.as_widget());
            let node = element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, Size::new(width, 300.0)),
            );
            let mut probe = TextBounds::default();
            element
                .as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
            let text = probe.0.expect("native text geometry");
            let cell = &node.children()[0];
            let trailing = cell.children()[1].bounds();
            assert!(
                (trailing.x + trailing.width - width).abs() < 0.1,
                "the trailing slot stays at the row's right edge"
            );
            assert!(text.x + text.width <= trailing.x + 0.1);
            if wraps {
                assert!(
                    text.height > natural.height + 1.0,
                    "an unbroken long label wraps at the actual narrow offer"
                );
            } else {
                assert!(
                    (text.height - natural.height).abs() < 0.1,
                    "Permission mode fits on one native line at its measured preferred width; already_fills={already_fills}, text={text:?}, natural={natural:?}"
                );
                assert!(text.width + 0.1 >= natural.width);
            }
        }
    }
}

#[test]
fn shared_row_states_use_the_same_wash_and_keep_target_and_selection_independent() {
    use cosmic::iced::widget::button::{Catalog, Status};
    for theme in [cosmic::Theme::light(), cosmic::Theme::dark()] {
        let normal = row_style(false, false, RowContour::Rounded);
        let selected = row_style(true, false, RowContour::Rounded);
        let style = |class: &cosmic::theme::style::iced::Button, state| {
            Catalog::style(&theme, class, state)
        };
        assert_eq!(style(&normal, Status::Active).background, None);
        assert_eq!(
            style(&normal, Status::Hovered).background,
            style(&selected, Status::Active).background
        );
        assert_eq!(
            style(&selected, Status::Hovered).background,
            style(&selected, Status::Active).background
        );
        assert_eq!(
            style(&selected, Status::Active).text_color,
            theme.cosmic().accent_text_color().into()
        );
        let open = row_style(false, true, RowContour::OpenTitle);
        let rest = style(&open, Status::Active);
        assert_eq!(rest.background, Some(crate::muted_colours(&theme).0.into()));
        assert_eq!(rest.text_color, crate::muted_colours(&theme).1);
        assert_ne!(style(&open, Status::Hovered).background, rest.background);
        assert_ne!(style(&open, Status::Pressed).background, rest.background);
        let disabled = style(&selected, Status::Disabled);
        assert!(disabled.text_color.a < rest.text_color.a);
        assert_ne!(
            disabled.text_color,
            style(&selected, Status::Active).text_color
        );
        let contour = RowContour::FrameBottom { inset: 2.0 };
        let border = contour.target_border(&theme);
        let selected = Catalog::style(&theme, &row_style(true, false, contour), Status::Active);
        assert_eq!(
            selected.border.radius, border.radius,
            "wash and target share their contour"
        );
        let [tl, tr, br, bl] = theme.cosmic().corner_radii.radius_s;
        assert_eq!(
            <[f32; 4]>::from(border.radius),
            [tl, tr, (br - 2.0).max(0.0), (bl - 2.0).max(0.0)]
        );
        assert_eq!(border.width, 1.0);
        assert_eq!(
            <[f32; 4]>::from(RowContour::FrameBottom { inset: 100.0 }.radius(&theme)),
            [tl, tr, 0.0, 0.0]
        );
    }
}

#[test]
fn native_selected_row_keeps_its_wash_with_a_keyboard_target_but_mouse_focus_adds_no_border() {
    use cosmic::iced::advanced::{
        renderer,
        widget::{Id, operation},
    };
    use cosmic::iced::{Color, Length};
    let id = Id::new("selected-native-target");
    let theme = cosmic::Theme::light();
    let paint = |keyboard_target: bool| {
        let mut renderer = renderer();
        let button = widget::button(space(40.0))
            .width(Length::Fill)
            .height(40.0)
            .on_press(1)
            .class(row_style(
                true,
                false,
                RowContour::FrameBottom { inset: 2.0 },
            ));
        let mut row: Element<'_, u8> =
            row_target(id.clone(), button, RowContour::FrameBottom { inset: 2.0 })
                .on_activate(1)
                .into();
        let mut tree = Tree::new(row.as_widget());
        let node = row.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(120.0, 40.0)),
        );
        row.as_widget_mut().operate(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &mut operation::focusable::focus::<()>(id.clone()),
        );
        row.as_widget_mut().operate(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &mut crate::popup::show_keyboard_target(id.clone(), keyboard_target),
        );
        row.as_widget_mut().update(
            &mut tree,
            &Event::Window(cosmic::iced::window::Event::RedrawRequested(
                std::time::Instant::now(),
            )),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut cosmic::iced::advanced::clipboard::Null,
            &mut Shell::new(&mut Vec::new()),
            &Rectangle::with_size(Size::new(120.0, 40.0)),
        );
        row.as_widget().draw(
            &tree,
            &mut renderer,
            &theme,
            &renderer::Style::default(),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &Rectangle::with_size(Size::new(120.0, 40.0)),
        );
        renderer.screenshot(Size::new(120, 40), 1.0, Color::WHITE)
    };
    let pointer = paint(false);
    let keyboard = paint(true);
    let pixel = |pixels: &[u8], x: usize, y: usize| pixels[(y * 120 + x) * 4..][..4].to_vec();
    assert_eq!(
        pixel(&pointer, 60, 30),
        pixel(&keyboard, 60, 30),
        "targeting does not replace selection"
    );
    assert_ne!(
        pixel(&pointer, 0, 20),
        pixel(&keyboard, 0, 20),
        "only keyboard targeting draws the row border"
    );
    assert_eq!(
        pixel(&keyboard, 0, 20),
        vec![82, 82, 82, 255],
        "the border is household Dark, not accent"
    );
}

#[test]
fn embedded_native_input_receives_space_before_the_target_activation_adapter() {
    use cosmic::iced::advanced::widget::{Id, Operation, operation};
    use cosmic::iced::keyboard;
    let renderer = renderer();
    let outer = Id::new("input-row");
    let inner = Id::new("embedded-input");
    let input = widget::text_input("", "").id(inner).on_input(|_| 8);
    let mut row: Element<'_, u8> = row_target(outer, input, RowContour::Rounded)
        .on_activate(7)
        .into();
    let mut tree = Tree::new(row.as_widget());
    let node = row.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(240.0, 40.0)),
    );
    struct FocusComposite;
    impl Operation for FocusComposite {
        fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
            visit(self);
        }
        fn focusable(
            &mut self,
            _: Option<&Id>,
            _: Rectangle,
            state: &mut dyn operation::Focusable,
        ) {
            state.focus();
        }
    }
    row.as_widget_mut().operate(
        &mut tree,
        Layout::new(&node),
        &renderer,
        &mut FocusComposite,
    );
    let event = Event::Keyboard(keyboard::Event::KeyPressed {
        key: keyboard::Key::Character(" ".into()),
        modified_key: keyboard::Key::Character(" ".into()),
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Space),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::empty(),
        text: Some(" ".into()),
        repeat: false,
    });
    let mut messages = Vec::new();
    row.as_widget_mut().update(
        &mut tree,
        &event,
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut Shell::new(&mut messages),
        &Rectangle::with_size(Size::new(240.0, 40.0)),
    );
    assert_eq!(
        messages,
        [8],
        "editing space does not activate the surrounding row"
    );
}
