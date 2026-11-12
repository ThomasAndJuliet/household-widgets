// SPDX-License-Identifier: GPL-3.0-only
//! Exercise information ownership through native UserInterface dispatch.
use super::*;
use crate::layout::{Alignment, AnchorCandidate, Level, Side, child};
use cosmic::iced::advanced::renderer::Headless;
use cosmic::iced::runtime::{UserInterface, user_interface};
use cosmic::iced::{Font, Pixels, Point, window};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
enum Message {
    Activate,
    Dismiss,
}
fn renderer() -> Renderer {
    cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
        Font::default(),
        Pixels(14.0),
        Some("tiny-skia"),
    ))
    .unwrap()
}
fn key(key: keyboard::Key, modifiers: keyboard::Modifiers) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        modified_key: key.clone(),
        key,
        modifiers,
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyH),
        location: keyboard::Location::Standard,
        text: None,
        repeat: false,
    })
}
fn named(key_name: keyboard::key::Named) -> Event {
    key(keyboard::Key::Named(key_name), keyboard::Modifiers::empty())
}
fn redraw(at: Instant) -> Event {
    Event::Window(window::Event::RedrawRequested(at))
}
fn view(anchors: &Anchors, scroll: bool) -> Host<'static, Message> {
    let render = anchors.clone();
    let count = if scroll { 16 } else { 2 };
    let content = child(move |_| {
        let rows = (0..count)
            .map(|at| {
                let id = Id::new(format!("information-row-{at}"));
                let target = focus_target(id.clone(), widget::space().width(120).height(32));
                let target: Element<'static, Message> = if at == 0 {
                    target.on_activate(Message::Activate).into()
                } else {
                    target.into()
                };
                crate::hover_card::hover_box(
                    render.clone(),
                    id,
                    target,
                    child(|_| widget::text("Shared explanation").into()),
                    vec![AnchorCandidate::new(Side::Right, Alignment::StartBounded)],
                )
            })
            .collect::<Vec<_>>();
        let rows = widget::column(rows);
        if scroll {
            cosmic::iced::widget::scrollable(rows)
                .id("information-scroll")
                .height(100)
                .width(140)
                .into()
        } else {
            rows.into()
        }
    });
    let panel = Panel::new(
        Attachment::below(Rectangle::new(
            Point::new(20.0, 20.0),
            Size::new(120.0, 20.0),
        )),
        vec![Stage::new(Level::Preferred, content)],
    )
    .focus_first()
    .focus_targets(
        (0..count)
            .map(|at| {
                let target = FocusTarget::new(format!("information-row-{at}"));
                if at == 0 {
                    target.activate(Message::Activate)
                } else {
                    target
                }
            })
            .collect(),
    );
    host(widget::space().width(Length::Fill).height(Length::Fill))
        .anchors(anchors.clone())
        .chain(vec![panel])
        .on_dismiss(Message::Dismiss)
}
fn update(
    ui: &mut UserInterface<'_, Message, Theme, Renderer>,
    renderer: &mut Renderer,
    event: Event,
    cursor: mouse::Cursor,
) -> Vec<Message> {
    let mut messages = Vec::new();
    ui.update(
        &[event],
        cursor,
        renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut messages,
    );
    messages
}
fn focused(
    ui: &mut UserInterface<'_, Message, Theme, Renderer>,
    renderer: &Renderer,
) -> Option<Id> {
    let mut probe = FocusProbe::default();
    ui.operate(renderer, &mut probe);
    probe.focused
}

#[test]
fn native_disabled_navigation_delays_information_without_pointer_reclaim_or_activation() {
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let mut ui = UserInterface::build(
        view(&anchors, false),
        Size::new(600.0, 400.0),
        user_interface::Cache::new(),
        &mut renderer,
    );
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now()),
        mouse::Cursor::Unavailable,
    );
    let first = Id::new("information-row-0");
    let second = Id::new("information-row-1");
    assert_eq!(focused(&mut ui, &renderer), Some(first.clone()));
    assert_eq!(
        anchors.hovered(),
        None,
        "structural initial focus does not create information ownership"
    );
    let p = anchors.visible(&first).unwrap().center();
    let cursor = mouse::Cursor::Available(p);
    update(
        &mut ui,
        &mut renderer,
        Event::Mouse(mouse::Event::CursorMoved { position: p }),
        cursor,
    );
    assert_eq!(anchors.hovered(), Some(first.clone()));
    update(
        &mut ui,
        &mut renderer,
        named(keyboard::key::Named::ArrowDown),
        cursor,
    );
    assert_eq!(focused(&mut ui, &renderer), Some(second.clone()));
    assert_eq!(anchors.hovered(), Some(second.clone()));
    assert!(!anchors.information_visible(&second));
    let now = Instant::now();
    update(
        &mut ui,
        &mut renderer,
        redraw(now + INFORMATION_DELAY / 2),
        cursor,
    );
    assert!(!anchors.information_visible(&second));
    update(
        &mut ui,
        &mut renderer,
        redraw(now + INFORMATION_DELAY),
        cursor,
    );
    assert!(
        anchors.information_visible(&second),
        "stationary pointer cannot reclaim keyboard target on redraw"
    );
    assert!(
        update(
            &mut ui,
            &mut renderer,
            named(keyboard::key::Named::Enter),
            cursor
        )
        .is_empty(),
        "disabled row cannot activate"
    );
    update(
        &mut ui,
        &mut renderer,
        key(
            keyboard::Key::Character("x".into()),
            keyboard::Modifiers::empty(),
        ),
        cursor,
    );
    assert_eq!(
        anchors.hovered(),
        Some(second.clone()),
        "ordinary typing does not retarget"
    );
    let moved = Point::new(p.x + 1.0, p.y);
    update(
        &mut ui,
        &mut renderer,
        Event::Mouse(mouse::Event::CursorMoved { position: moved }),
        mouse::Cursor::Available(moved),
    );
    assert_eq!(anchors.hovered(), Some(first.clone()));
    assert_eq!(
        focused(&mut ui, &renderer),
        Some(second.clone()),
        "information pointer ownership does not move keyboard focus"
    );
    update(
        &mut ui,
        &mut renderer,
        key(
            keyboard::Key::Character("h".into()),
            keyboard::Modifiers::ALT,
        ),
        cursor,
    );
    assert!(
        anchors.information_visible(&second),
        "Alt+H immediately uses current keyboard target's same card"
    );
    assert!(
        update(
            &mut ui,
            &mut renderer,
            named(keyboard::key::Named::Escape),
            cursor
        )
        .is_empty(),
        "Escape closes the card before menu"
    );
    assert!(!anchors.information_visible(&second));
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now() + INFORMATION_DELAY * 4),
        cursor,
    );
    assert!(
        !anchors.information_visible(&second),
        "dismissed card does not reopen on stale timer"
    );
    update(
        &mut ui,
        &mut renderer,
        key(
            keyboard::Key::Character("h".into()),
            keyboard::Modifiers::ALT,
        ),
        cursor,
    );
    assert!(anchors.information_visible(&second));
    update(
        &mut ui,
        &mut renderer,
        named(keyboard::key::Named::Escape),
        cursor,
    );
    assert_eq!(
        update(
            &mut ui,
            &mut renderer,
            named(keyboard::key::Named::Escape),
            cursor
        ),
        [Message::Dismiss]
    );
}

#[test]
fn native_deactivation_cancels_pending_information_and_dismisses_menu() {
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let mut ui = UserInterface::build(
        view(&anchors, false),
        Size::new(600.0, 400.0),
        user_interface::Cache::new(),
        &mut renderer,
    );
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now()),
        mouse::Cursor::Unavailable,
    );
    update(
        &mut ui,
        &mut renderer,
        named(keyboard::key::Named::ArrowDown),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(anchors.hovered(), Some(Id::new("information-row-1")));
    assert_eq!(
        update(
            &mut ui,
            &mut renderer,
            Event::Window(window::Event::Unfocused),
            mouse::Cursor::Unavailable
        ),
        [Message::Dismiss]
    );
    update(
        &mut ui,
        &mut renderer,
        Event::Window(window::Event::Focused),
        mouse::Cursor::Unavailable,
    );
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now() + INFORMATION_DELAY * 4),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(anchors.hovered(), None);
}

#[test]
fn native_navigation_reveals_disabled_offscreen_target_before_information_opens() {
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let mut ui = UserInterface::build(
        view(&anchors, true),
        Size::new(600.0, 400.0),
        user_interface::Cache::new(),
        &mut renderer,
    );
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now()),
        mouse::Cursor::Unavailable,
    );
    let last = Id::new("information-row-15");
    assert_eq!(anchors.visible(&last), None);
    update(
        &mut ui,
        &mut renderer,
        named(keyboard::key::Named::End),
        mouse::Cursor::Unavailable,
    );
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now()),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(focused(&mut ui, &renderer), Some(last.clone()));
    assert!(
        anchors.visible(&last).is_some(),
        "native scroll follows a disabled navigation target"
    );
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now() + INFORMATION_DELAY),
        mouse::Cursor::Unavailable,
    );
    assert!(anchors.information_visible(&last));
    assert!(
        update(
            &mut ui,
            &mut renderer,
            named(keyboard::key::Named::Enter),
            mouse::Cursor::Unavailable
        )
        .is_empty()
    );
}

#[test]
fn native_external_open_source_cancels_information_until_new_targeting_after_close() {
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let source = Id::new("external-native-menu-source");
    let make = |open: bool| {
        let target = anchors.trigger(
            source.clone(),
            focus_target(
                "external-native-control",
                widget::space().width(80).height(32),
            ),
        );
        let hover = crate::hover_card::hover_box(
            anchors.clone(),
            source.clone(),
            target,
            child(|_| widget::text("Same explanation").into()),
            vec![AnchorCandidate::new(Side::Below, Alignment::StartBounded)],
        );
        host(hover).anchors(anchors.clone()).open_sources(if open {
            vec![source.clone()]
        } else {
            Vec::new()
        })
    };
    let size = Size::new(400.0, 300.0);
    let cursor = mouse::Cursor::Available(Point::new(10.0, 10.0));
    let moved = || {
        Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(10.0, 10.0),
        })
    };
    let mut ui = UserInterface::build(
        make(false),
        size,
        user_interface::Cache::new(),
        &mut renderer,
    );
    update(&mut ui, &mut renderer, redraw(Instant::now()), cursor);
    ui.operate(
        &renderer,
        &mut operation::focusable::focus::<()>(Id::new("external-native-control")),
    );
    update(&mut ui, &mut renderer, moved(), cursor);
    assert_eq!(anchors.hovered(), Some(source.clone()));
    let mut ui = UserInterface::build(make(true), size, ui.into_cache(), &mut renderer);
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now() + INFORMATION_DELAY * 3),
        cursor,
    );
    assert!(anchors.is_open(&source));
    assert_eq!(anchors.hovered(), None);
    assert_eq!(
        focused(&mut ui, &renderer),
        Some(Id::new("external-native-control")),
        "source suppression preserves its native focus subtree"
    );
    update(
        &mut ui,
        &mut renderer,
        key(
            keyboard::Key::Character("h".into()),
            keyboard::Modifiers::ALT,
        ),
        cursor,
    );
    assert!(!anchors.information_visible(&source));
    let mut ui = UserInterface::build(make(false), size, ui.into_cache(), &mut renderer);
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now() + INFORMATION_DELAY * 4),
        cursor,
    );
    assert!(
        !anchors.information_visible(&source),
        "closing another native surface cannot resurrect the old timer"
    );
    update(&mut ui, &mut renderer, moved(), cursor);
    assert!(
        !anchors.information_visible(&source),
        "new targeting restarts the shared delay"
    );
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now() + INFORMATION_DELAY),
        cursor,
    );
    assert!(anchors.information_visible(&source));
}

#[derive(Default)]
struct TargetPresentation(Vec<Id>);
impl Operation for TargetPresentation {
    fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
        visit(self);
    }
    fn custom(&mut self, id: Option<&Id>, _: Rectangle, state: &mut dyn std::any::Any) {
        if focus::keyboard_target_visible(state) == Some(true)
            && let Some(id) = id
        {
            self.0.push(id.clone());
        }
    }
}
fn presentation(
    ui: &mut UserInterface<'_, Message, Theme, Renderer>,
    renderer: &Renderer,
) -> Vec<Id> {
    let mut probe = TargetPresentation::default();
    ui.operate(renderer, &mut probe);
    probe.0
}

#[test]
fn native_target_modality_survives_beside_inline_composition_and_target_remapping() {
    use crate::layout::Side;
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let render_row = |name: &'static str| {
        child(move |_| focus_target(name, widget::space().width(120).height(32)).into())
    };
    let make = |remove_first: bool| {
        let parent = anchors.clone();
        let root = child(move |_| {
            parent.anchor(
                "submenu-owning-row",
                focus_target("root-row", widget::space().width(100).height(32)),
            )
        });
        let child_rows = if remove_first {
            vec![render_row("remaining-row")]
        } else {
            vec![render_row("removed-row"), render_row("remaining-row")]
        };
        let child_content = crate::layout::column_child(child_rows, 0.0);
        let inline = crate::layout::column_child(vec![root.clone(), child_content.clone()], 0.0);
        let first = Panel::new(
            Attachment::below(Rectangle::new(
                Point::new(20.0, 20.0),
                Size::new(100.0, 20.0),
            )),
            vec![Stage::new(Level::Preferred, root)],
        )
        .focus_first()
        .focus_targets(vec![FocusTarget::new("root-row")])
        .embedded_compositions(vec![vec![Stage::new(Level::Preferred, inline)]]);
        let second = Panel::new(
            Attachment::new(
                Rectangle::default(),
                vec![AnchorCandidate::new(Side::Right, Alignment::Start)],
            ),
            vec![Stage::new(Level::Preferred, child_content)],
        )
        .source("submenu-owning-row")
        .beside_previous_surface()
        .focus_first()
        .focus_targets(if remove_first {
            vec![FocusTarget::new("remaining-row")]
        } else {
            vec![
                FocusTarget::new("removed-row"),
                FocusTarget::new("remaining-row"),
            ]
        });
        host(widget::space().width(Length::Fill).height(Length::Fill))
            .anchors(anchors.clone())
            .chain(vec![first, second])
            .on_dismiss(Message::Dismiss)
    };
    let mut ui = UserInterface::build(
        make(false),
        Size::new(600.0, 400.0),
        user_interface::Cache::new(),
        &mut renderer,
    );
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now()),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(anchors.surfaces().len(), 2);
    assert!(
        presentation(&mut ui, &renderer).is_empty(),
        "initial structural focus has no keyboard border"
    );
    // Home deliberately targets the existing first row, including its modality.
    update(
        &mut ui,
        &mut renderer,
        named(keyboard::key::Named::Home),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(presentation(&mut ui, &renderer), [Id::new("removed-row")]);
    let mut ui = UserInterface::build(
        make(false),
        Size::new(180.0, 400.0),
        ui.into_cache(),
        &mut renderer,
    );
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now()),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(anchors.surfaces().len(), 1);
    assert_eq!(
        presentation(&mut ui, &renderer),
        [Id::new("removed-row")],
        "resize preserves the deliberate target contour in inline composition"
    );
    let mut ui = UserInterface::build(
        make(false),
        Size::new(600.0, 400.0),
        ui.into_cache(),
        &mut renderer,
    );
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now()),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(anchors.surfaces().len(), 2);
    assert_eq!(presentation(&mut ui, &renderer), [Id::new("removed-row")]);
    let mut ui = UserInterface::build(
        make(true),
        Size::new(600.0, 400.0),
        ui.into_cache(),
        &mut renderer,
    );
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now()),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(
        focused(&mut ui, &renderer),
        Some(Id::new("remaining-row")),
        "stable list remapping restores actual native focus to the survivor"
    );
    assert_eq!(presentation(&mut ui, &renderer), [Id::new("remaining-row")]);
}

#[test]
fn native_scrolling_current_source_away_clears_card_without_resurrection_on_return() {
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let mut ui = UserInterface::build(
        view(&anchors, true),
        Size::new(600.0, 400.0),
        user_interface::Cache::new(),
        &mut renderer,
    );
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now()),
        mouse::Cursor::Unavailable,
    );
    update(
        &mut ui,
        &mut renderer,
        named(keyboard::key::Named::End),
        mouse::Cursor::Unavailable,
    );
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now() + INFORMATION_DELAY),
        mouse::Cursor::Unavailable,
    );
    let last = Id::new("information-row-15");
    assert!(anchors.information_visible(&last));
    ui.operate(
        &renderer,
        &mut operation::scrollable::snap_to::<()>(
            Id::new("information-scroll"),
            operation::scrollable::RelativeOffset {
                x: None,
                y: Some(0.0),
            },
        ),
    );
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now() + INFORMATION_DELAY * 2),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(anchors.visible(&last), None);
    assert_eq!(anchors.hovered(), None);
    ui.operate(
        &renderer,
        &mut operation::scrollable::snap_to::<()>(
            Id::new("information-scroll"),
            operation::scrollable::RelativeOffset {
                x: None,
                y: Some(1.0),
            },
        ),
    );
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now() + INFORMATION_DELAY * 3),
        mouse::Cursor::Unavailable,
    );
    assert!(anchors.visible(&last).is_some());
    assert!(!anchors.information_visible(&last));
    assert_eq!(
        anchors.hovered(),
        None,
        "returning source cannot revive an earlier information owner"
    );
}

#[test]
fn native_keyboard_opening_targets_first_disabled_entry_while_mouse_opening_stays_structural() {
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let information_row = |id: &'static str, render: Anchors, action: bool| {
        child(move |_| {
            let native = focus_target(id, widget::space().width(130).height(32));
            let native: Element<'static, Message> = if action {
                native.on_activate(Message::Activate).into()
            } else {
                native.into()
            };
            crate::hover_card::hover_box(
                render.clone(),
                id,
                native,
                child(|_| widget::text("Disabled explanation").into()),
                vec![AnchorCandidate::new(Side::Right, Alignment::StartBounded)],
            )
        })
    };
    let make = |depth: usize| {
        let trigger = anchors.trigger(
            "keyboard-trigger",
            focus_target("keyboard-trigger", widget::space().width(80).height(32))
                .on_activate(Message::Activate),
        );
        let mut panels = Vec::new();
        if depth > 0 {
            let root = crate::layout::column_child(
                vec![
                    information_row("keyboard-disabled", anchors.clone(), false),
                    information_row("keyboard-folder", anchors.clone(), true),
                ],
                0.0,
            );
            panels.push(
                Panel::new(
                    Attachment::below(Rectangle::default()),
                    vec![Stage::new(Level::Preferred, root)],
                )
                .source("keyboard-trigger")
                .focus_first()
                .focus_targets(vec![
                    FocusTarget::new("keyboard-disabled"),
                    FocusTarget::new("keyboard-folder")
                        .activate(Message::Activate)
                        .open(Message::Activate),
                ]),
            );
        }
        if depth > 1 {
            panels.push(
                Panel::new(
                    Attachment::beside(Rectangle::default()),
                    vec![Stage::new(
                        Level::Preferred,
                        information_row("keyboard-child-disabled", anchors.clone(), false),
                    )],
                )
                .source("keyboard-folder")
                .beside_previous_surface()
                .focus_first()
                .focus_targets(vec![FocusTarget::new("keyboard-child-disabled")]),
            );
        }
        host(trigger)
            .anchors(anchors.clone())
            .chain(panels)
            .on_dismiss(Message::Dismiss)
    };
    let size = Size::new(600.0, 400.0);
    let mut ui = UserInterface::build(make(0), size, user_interface::Cache::new(), &mut renderer);
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now()),
        mouse::Cursor::Unavailable,
    );
    ui.operate(
        &renderer,
        &mut operation::focusable::focus::<()>(Id::new("keyboard-trigger")),
    );
    assert_eq!(
        update(
            &mut ui,
            &mut renderer,
            named(keyboard::key::Named::Enter),
            mouse::Cursor::Unavailable
        ),
        [Message::Activate]
    );
    let mut ui = UserInterface::build(make(1), size, ui.into_cache(), &mut renderer);
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now()),
        mouse::Cursor::Unavailable,
    );
    let first = Id::new("keyboard-disabled");
    assert_eq!(focused(&mut ui, &renderer), Some(first.clone()));
    assert_eq!(
        presentation(&mut ui, &renderer).as_slice(),
        std::slice::from_ref(&first)
    );
    assert_eq!(anchors.hovered(), Some(first.clone()));
    assert!(!anchors.information_visible(&first));
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now() + INFORMATION_DELAY),
        mouse::Cursor::Unavailable,
    );
    assert!(anchors.information_visible(&first));
    assert!(
        update(
            &mut ui,
            &mut renderer,
            named(keyboard::key::Named::Enter),
            mouse::Cursor::Unavailable
        )
        .is_empty()
    );
    update(
        &mut ui,
        &mut renderer,
        named(keyboard::key::Named::ArrowDown),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(
        update(
            &mut ui,
            &mut renderer,
            named(keyboard::key::Named::ArrowRight),
            mouse::Cursor::Unavailable
        ),
        [Message::Activate]
    );
    let mut ui = UserInterface::build(make(2), size, ui.into_cache(), &mut renderer);
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now()),
        mouse::Cursor::Unavailable,
    );
    let child = Id::new("keyboard-child-disabled");
    assert_eq!(focused(&mut ui, &renderer), Some(child.clone()));
    assert_eq!(
        presentation(&mut ui, &renderer).as_slice(),
        std::slice::from_ref(&child)
    );
    assert_eq!(anchors.hovered(), Some(child.clone()));
    assert!(!anchors.information_visible(&child));
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now() + INFORMATION_DELAY),
        mouse::Cursor::Unavailable,
    );
    assert!(anchors.information_visible(&child));
    // A separate mouse opening carries no one-shot keyboard entry intent.
    let mut ui = UserInterface::build(make(0), size, user_interface::Cache::new(), &mut renderer);
    update(
        &mut ui,
        &mut renderer,
        redraw(Instant::now()),
        mouse::Cursor::Unavailable,
    );
    let cursor = mouse::Cursor::Available(Point::new(10.0, 10.0));
    update(
        &mut ui,
        &mut renderer,
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        cursor,
    );
    let mut ui = UserInterface::build(make(1), size, ui.into_cache(), &mut renderer);
    update(&mut ui, &mut renderer, redraw(Instant::now()), cursor);
    assert_eq!(focused(&mut ui, &renderer), Some(first));
    assert!(presentation(&mut ui, &renderer).is_empty());
    assert_eq!(anchors.hovered(), None);
}
