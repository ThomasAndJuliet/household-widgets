// SPDX-License-Identifier: GPL-3.0-only

//! Exercise the real Iced update/layout/overlay/draw lifecycle. Unlike a
//! handcrafted overlay harness, UserInterface recreates and lays out overlay
//! wrappers at each update, then recreates the wrapper without layout at draw.

use super::*;
use crate::layout::{Level, child};
use cosmic::iced::advanced::renderer::Headless;
use cosmic::iced::runtime::{UserInterface, user_interface};
use cosmic::iced::widget as native;
use cosmic::iced::{Color, Font, Pixels, Point, window};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn real_user_interface_retains_button_status_and_requests_hover_redraw() {
    let mut renderer = cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
        Font::default(),
        Pixels(14.0),
        Some("tiny-skia"),
    ))
    .expect("headless tiny-skia renderer");
    let drawn = Rc::new(RefCell::new(Vec::new()));
    let observed = drawn.clone();
    let panel = Panel::new(
        Attachment::below(Rectangle::new(
            Point::new(20.0, 0.0),
            Size::new(100.0, 20.0),
        )),
        vec![Stage::new(
            Level::Preferred,
            child(move |_| {
                let observed = observed.clone();
                native::button(native::space().width(100).height(30))
                    .padding(0)
                    .on_press(())
                    .class(cosmic::theme::style::iced::Button::Custom(Box::new(
                        move |_, status| {
                            observed.borrow_mut().push(status);
                            native::button::Style::default()
                        },
                    )))
                    .into()
            }),
        )],
    );
    let root = host(widget::space().width(Length::Fill).height(Length::Fill)).chain(vec![panel]);
    let mut ui = UserInterface::build(
        root,
        Size::new(300.0, 200.0),
        user_interface::Cache::new(),
        &mut renderer,
    );
    let outside = mouse::Cursor::Available(Point::new(250.0, 150.0));
    let inside = mouse::Cursor::Available(Point::new(30.0, 30.0));
    let redraw = || Event::Window(window::Event::RedrawRequested(std::time::Instant::now()));
    let mut messages = Vec::new();
    let mut clipboard = cosmic::iced::advanced::clipboard::Null;
    let style = renderer::Style {
        icon_color: Color::BLACK,
        text_color: Color::BLACK,
        scale_factor: 1.0,
    };
    ui.update(
        &[redraw()],
        outside,
        &mut renderer,
        &mut clipboard,
        &mut messages,
    );
    ui.draw(&mut renderer, &Theme::light(), &style, outside);
    assert_eq!(
        drawn.borrow().last(),
        Some(&native::button::Status::Active),
        "draw reconstructs the overlay wrapper without losing native child status"
    );

    let (state, _) = ui.update(
        &[Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(30.0, 30.0),
        })],
        inside,
        &mut renderer,
        &mut clipboard,
        &mut messages,
    );
    assert!(
        matches!(
            state,
            user_interface::State::Updated {
                redraw_request: window::RedrawRequest::NextFrame,
                ..
            }
        ),
        "moving onto a native button compares its previous Active status and requests a new frame"
    );
    ui.update(
        &[redraw()],
        inside,
        &mut renderer,
        &mut clipboard,
        &mut messages,
    );
    ui.draw(&mut renderer, &Theme::light(), &style, inside);
    assert_eq!(
        drawn.borrow().last(),
        Some(&native::button::Status::Hovered)
    );

    ui.update(
        &[
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            redraw(),
        ],
        inside,
        &mut renderer,
        &mut clipboard,
        &mut messages,
    );
    ui.draw(&mut renderer, &Theme::light(), &style, inside);
    assert_eq!(
        drawn.borrow().last(),
        Some(&native::button::Status::Pressed)
    );
    ui.update(
        &[Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        ))],
        inside,
        &mut renderer,
        &mut clipboard,
        &mut messages,
    );
    assert_eq!(
        messages,
        vec![()],
        "the same retained native button still dispatches its action exactly once"
    );
}
