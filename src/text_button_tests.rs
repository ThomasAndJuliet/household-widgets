// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use cosmic::iced::advanced::layout;
use cosmic::iced::advanced::renderer::Headless;
use cosmic::iced::advanced::widget::Tree;
use cosmic::iced::{Font, Pixels, Size};

fn renderer() -> cosmic::Renderer {
    cosmic::iced::futures::executor::block_on(<cosmic::Renderer as Headless>::new(
        Font::default(),
        Pixels(14.0),
        Some("tiny-skia"),
    ))
    .expect("native renderer")
}

fn layout_of(mut element: Element<'_, ()>, renderer: &cosmic::Renderer) -> layout::Node {
    let mut tree = Tree::new(element.as_widget());
    element.as_widget_mut().layout(
        &mut tree,
        renderer,
        &layout::Limits::new(Size::ZERO, Size::new(500.0, 100.0)),
    )
}

fn mark() -> Element<'static, ()> {
    widget::container(widget::Space::new().width(12).height(12))
        .style(|_| cosmic::widget::container::Style {
            background: Some(cosmic::iced::Color::from_rgb(1.0, 0.0, 0.0).into()),
            ..Default::default()
        })
        .into()
}

#[test]
fn tertiary_native_geometry_and_compact_content() {
    let renderer = renderer();
    let build = || {
        Tertiary::new("A long accessible name", theme::Button::default(), ())
            .leading_element(mark())
            .on_press(())
    };
    let default = layout_of(build().into(), &renderer);
    assert_eq!(
        default.size().height,
        f32::from(theme::active().cosmic().spacing.space_l)
    );
    let labelled = layout_of(build().height(32.0).into(), &renderer);
    assert_eq!(labelled.size().height, 32.0);
    let compact = layout_of(build().height(32.0).icon_only(true).into(), &renderer);
    assert_eq!(compact.size(), Size::new(32.0, 32.0));
    assert!(labelled.size().width > compact.size().width);
    let row = &compact.children()[0];
    assert_eq!(
        row.children().len(),
        1,
        "compact row has only the mark, with no label or gap"
    );
    assert_eq!(row.children()[0].size(), Size::new(12.0, 12.0));
}

#[test]
fn tertiary_compact_draws_custom_colour_and_responds_to_click() {
    use cosmic::iced::advanced::{clipboard, renderer::Style};
    use cosmic::iced::runtime::user_interface::{Cache, UserInterface};
    use cosmic::iced::{Color, Event, Point, mouse};

    let mut renderer = renderer();
    let element: Element<'_, ()> = Tertiary::new("Named action", theme::Button::default(), ())
        .leading_icon(widget::icon::from_name("missing-symbolic").handle())
        .leading_element(mark())
        .height(32.0)
        .icon_only(true)
        .on_press(())
        .into();
    let mut ui = UserInterface::build(
        element,
        Size::new(32.0, 32.0),
        Cache::default(),
        &mut renderer,
    );
    let cursor = mouse::Cursor::Available(Point::new(16.0, 16.0));
    let events = [
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ];
    let mut messages = Vec::new();
    let _ = ui.update(
        &events,
        cursor,
        &mut renderer,
        &mut clipboard::Null,
        &mut messages,
    );
    assert_eq!(messages.len(), 1);
    ui.draw(
        &mut renderer,
        &cosmic::Theme::default(),
        &Style::default(),
        cursor,
    );
    let pixels = renderer.screenshot(Size::new(32, 32), 1.0, Color::BLACK);
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[0] > 240 && p[1] < 15 && p[2] < 15),
        "the custom red mark survives rendering and takes precedence over the icon"
    );
}
