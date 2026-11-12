// SPDX-License-Identifier: GPL-3.0-only
//! Native component proof: the same host and row composition serve actions and
//! choices, including an inline disclosure and an ordinary text input.
use crate::{layout as engine, menu_parts as parts, popup};
use cosmic::iced::advanced::{
    Layout, Widget, layout, mouse,
    renderer::{Headless, Style},
    widget::Tree,
};
use cosmic::iced::{Font, Length, Pixels, Point, Rectangle, Size, Vector};
use cosmic::{Element, Renderer, Theme, widget};

#[derive(Clone, Debug, PartialEq)]
enum Message {
    Action,
    Choice,
    More,
    Submenu,
    Input(String),
    Dismiss,
    Close(usize),
    NoFit(usize),
}

fn label(value: &str) -> Element<'static, Message> {
    widget::text(value.to_owned()).size(14).into()
}

fn row(
    name: &str,
    selected: bool,
    trailing: Option<&str>,
    message: Option<Message>,
) -> Element<'static, Message> {
    let content = parts::row_content(
        parts::RowSlots {
            mark_width: Some(16.0),
            icon_width: None,
            gap: 8.0,
        },
        selected.then(|| label("✓")),
        None,
        label(name),
        trailing.map(label),
    );
    let tip = if message.is_none() {
        parts::TooltipChoice::Tooltip {
            content: label("Select an item before using this action."),
            position: widget::tooltip::Position::Right,
        }
    } else {
        parts::TooltipChoice::NoTooltip
    };
    parts::action_row_with(content, tip, message, |button| button.padding([7, 8]))
}

fn card(choices: bool, expanded: bool) -> Element<'static, Message> {
    let first = if choices {
        vec![
            row("Standard", true, None, Some(Message::Choice)),
            row("Careful", false, None, Some(Message::Choice)),
        ]
    } else {
        vec![
            row(
                "New tab",
                false,
                Some("Ctrl + Shift + T"),
                Some(Message::Action),
            ),
            row("Copy selection", false, Some("Ctrl + C"), None),
        ]
    };
    let extra = parts::expandable(
        row(
            "More options",
            false,
            Some(if expanded { "−" } else { "+" }),
            Some(Message::More),
        ),
        expanded,
        vec![
            row("Compact view", false, None, Some(Message::Choice)),
            row("Detailed view", false, None, Some(Message::Choice)),
        ],
    );
    let input: Element<'_, Message> = widget::text_input("Filter options", "draft")
        .on_input(Message::Input)
        .width(Length::Fill)
        .into();
    let content = parts::groups(
        vec![
            first,
            vec![extra],
            vec![
                widget::container(input).padding([6, 8]).into(),
                row("Open profile", false, Some("›"), Some(Message::Submenu)),
            ],
        ],
        || widget::divider::horizontal::default().into(),
    );
    widget::container(content)
        .padding(6)
        .width(Length::Fill)
        .max_width(360)
        .class(cosmic::theme::Container::Dialog(true))
        .into()
}

fn stages(choices: bool, expanded: bool) -> Vec<engine::Stage<'static, Message>> {
    vec![engine::Stage::new(
        engine::Level::Preferred,
        engine::child(move |_| card(choices, expanded)).minimum_width(|_, _| 260.0),
    )]
}

fn renderer() -> Renderer {
    cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
        Font::default(),
        Pixels(14.0),
        Some("tiny-skia"),
    ))
    .expect("headless tiny-skia")
}

fn frame(choices: bool, expanded: bool, submenu: bool, size: Size) -> Vec<u8> {
    let mut renderer = renderer();
    let theme = Theme::light();
    let viewport = Rectangle::new(Point::ORIGIN, size);
    let anchor = Rectangle {
        x: 16.0,
        y: 12.0,
        width: 96.0,
        height: 32.0,
    };
    let attachment = engine::Attachment::below(anchor);
    let planned =
        engine::choose_attached(&stages(choices, expanded), &renderer, viewport, &attachment)
            .unwrap();
    assert!(planned.bounds.x >= 0.0 && planned.bounds.y >= 0.0);
    assert!(planned.bounds.x + planned.bounds.width <= size.width);
    assert!(planned.bounds.y + planned.bounds.height <= size.height);
    let mut chain = vec![popup::Panel::new(attachment, stages(choices, expanded))];
    if submenu {
        let source = Rectangle {
            x: 0.0,
            y: planned.bounds.height - 40.0,
            width: planned.bounds.width,
            height: 32.0,
        };
        chain.push(
            popup::Panel::new(
                engine::Attachment::beside(source),
                vec![engine::Stage::new(
                    engine::Level::Preferred,
                    engine::child(|_| {
                        widget::container(row("Default profile", true, None, Some(Message::Choice)))
                            .padding(6)
                            .width(180)
                            .class(cosmic::theme::Container::Dialog(true))
                            .into()
                    }),
                )],
            )
            .relative_to_previous(),
        );
    }
    let base: Element<'_, Message> =
        widget::container(widget::text(if choices { "Choices" } else { "File" }).size(14))
            .padding([20, 24])
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    let mut host = popup::host(base)
        .chain(chain)
        .on_dismiss(Message::Dismiss)
        .on_close(Message::Close)
        .on_no_fit(Message::NoFit);
    let mut tree = Tree::new(&host as &dyn Widget<Message, Theme, Renderer>);
    let node = host.layout(&mut tree, &renderer, &layout::Limits::new(size, size));
    let colour = theme.cosmic().on_bg_color().into();
    let style = Style {
        icon_color: colour,
        text_color: colour,
        scale_factor: 1.0,
    };
    host.draw(
        &tree,
        &mut renderer,
        &theme,
        &style,
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &viewport,
    );
    let mut overlay = host
        .overlay(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &viewport,
            Vector::ZERO,
        )
        .expect("open menu");
    let panel_node = overlay.as_overlay_mut().layout(&renderer, size);
    // Native buttons compute their active/disabled appearance on redraw.
    // Match the runtime's event-before-paint sequence rather than drawing a
    // newly constructed widget without its first event.
    let mut messages = Vec::new();
    overlay.as_overlay_mut().update(
        &cosmic::iced::Event::Window(cosmic::iced::window::Event::RedrawRequested(
            std::time::Instant::now(),
        )),
        Layout::new(&panel_node),
        mouse::Cursor::Unavailable,
        &renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut cosmic::iced::advanced::Shell::new(&mut messages),
    );
    overlay.as_overlay().draw(
        &mut renderer,
        &theme,
        &style,
        Layout::new(&panel_node),
        mouse::Cursor::Unavailable,
    );
    drop(overlay);
    renderer.screenshot(
        Size::new(size.width as u32, size.height as u32),
        1.0,
        theme.cosmic().bg_color().into(),
    )
}

#[test]
fn native_menu_fixture_renders_actions_choices_expansion_and_submenu() {
    for (name, choices, expanded, submenu, size) in [
        ("actions", false, false, true, Size::new(640.0, 380.0)),
        (
            "choices-expanded",
            true,
            true,
            false,
            Size::new(400.0, 400.0),
        ),
    ] {
        let rgba = frame(choices, expanded, submenu, size);
        assert_eq!(rgba.len(), size.width as usize * size.height as usize * 4);
        assert!(
            rgba.as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel.as_slice() != &rgba[..4])
        );
        if let Ok(directory) = std::env::var("HOUSEHOLD_MENU_FIXTURE_OUT") {
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                std::path::Path::new(&directory)
                    .join(format!("{name}-{}x{}.rgba", size.width, size.height)),
                rgba,
            )
            .unwrap();
        }
    }
}
