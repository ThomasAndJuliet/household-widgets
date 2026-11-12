// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::layout::{Level, child};
use cosmic::iced::advanced::renderer::Headless;
use cosmic::iced::runtime::{UserInterface, user_interface};
use cosmic::iced::widget as native;
use cosmic::iced::{Font, Pixels, Point, window};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Message {
    OpenA,
    OpenB,
    Dismiss,
    NoFit(usize),
    Action,
}

fn renderer() -> Renderer {
    cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
        Font::default(),
        Pixels(14.0),
        Some("tiny-skia"),
    ))
    .expect("headless tiny-skia renderer")
}
fn redraw() -> Event {
    Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))
}
fn panel() -> Panel<'static, Message> {
    Panel::new(
        Attachment::below(Rectangle::default()),
        vec![Stage::new(
            Level::Preferred,
            child(|_| {
                focus_target(
                    "menu-first",
                    native::button(native::space().width(80).height(40))
                        .padding(0)
                        .on_press(Message::Action),
                )
                .on_activate(Message::Action)
                .into()
            }),
        )],
    )
    .source("a")
    .key(1)
    .focus_first()
    .focus_targets(vec![
        FocusTarget::new("menu-first").activate(Message::Action),
    ])
}
fn view(anchors: &Anchors, open: bool, show_a: bool) -> Host<'static, Message> {
    let a = if show_a {
        anchors.trigger(
            "a",
            focus_target(
                "a",
                native::button(native::space().width(40).height(20))
                    .padding(0)
                    .on_press(Message::OpenA),
            )
            .on_activate(Message::OpenA),
        )
    } else {
        native::space().width(40).height(20).into()
    };
    let b = anchors.trigger(
        "b",
        focus_target(
            "b",
            native::button(native::space().width(40).height(20))
                .padding(0)
                .on_press(Message::OpenB),
        )
        .on_activate(Message::OpenB),
    );
    host(native::row(vec![a, native::space().width(40).into(), b]))
        .anchors(anchors.clone())
        .chain(if open { vec![panel()] } else { Vec::new() })
        .on_dismiss(Message::Dismiss)
        .on_no_fit(Message::NoFit)
}

#[test]
fn disposable_layout_queries_never_publish_source_geometry() {
    let renderer = renderer();
    let anchors = Anchors::default();
    let queried = anchors.clone();
    let component = child(move |_| -> Element<'static, ()> {
        queried.anchor("probe", native::space().width(80).height(20))
    });
    component
        .query_at(Level::Preferred, &renderer, Size::new(200.0, 100.0))
        .unwrap();
    assert_eq!(anchors.visible(&Id::new("probe")), None);
    assert!(anchors.surfaces().is_empty());
}

#[test]
fn registered_source_uses_translated_visible_clip_and_drops_hidden_geometry() {
    let renderer = renderer();
    let anchors = Anchors::default();
    let mut source: Element<'_, ()> =
        anchors.anchor("source", native::space().width(100).height(40));
    let mut tree = Tree::new(source.as_widget());
    let node = source
        .as_widget_mut()
        .layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(100.0, 40.0)),
        )
        .move_to(Point::new(20.0, 20.0));
    let clip = Rectangle::new(Point::new(30.0, 30.0), Size::new(50.0, 20.0));
    drop(source.as_widget_mut().overlay(
        &mut tree,
        Layout::new(&node),
        &renderer,
        &clip,
        Vector::new(10.0, 0.0),
    ));
    assert_eq!(anchors.visible(&Id::new("source")), Some(clip));
    anchors.begin(Vec::new());
    drop(source.as_widget_mut().overlay(
        &mut tree,
        Layout::new(&node),
        &renderer,
        &Rectangle::with_size(Size::new(5.0, 5.0)),
        Vector::ZERO,
    ));
    assert_eq!(anchors.visible(&Id::new("source")), None);
}

#[test]
fn live_source_reanchors_after_resize_and_hidden_source_is_explicit_no_fit() {
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let source = anchors.trigger("a", native::space().width(40).height(20));
    let root = host(
        native::row(vec![native::space().width(Length::Fill).into(), source]).width(Length::Fill),
    )
    .anchors(anchors.clone())
    .chain(vec![panel()])
    .on_no_fit(Message::NoFit);
    let mut ui = UserInterface::build(
        root,
        Size::new(300.0, 200.0),
        user_interface::Cache::new(),
        &mut renderer,
    );
    let mut messages = Vec::new();
    ui.update(
        &[redraw()],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut messages,
    );
    assert_eq!(anchors.visible(&Id::new("a")).unwrap().x, 260.0);
    assert_eq!(
        anchors.surfaces()[0].x,
        220.0,
        "end-alignment follows the actual source"
    );
    let mut ui = ui.relayout(Size::new(180.0, 120.0), &mut renderer);
    ui.update(
        &[redraw()],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut messages,
    );
    assert_eq!(anchors.visible(&Id::new("a")).unwrap().x, 140.0);
    assert_eq!(anchors.surfaces()[0].x, 100.0);
    assert!(messages.is_empty());
    let mut ui = UserInterface::build(
        view(&anchors, true, false),
        Size::new(180.0, 120.0),
        ui.into_cache(),
        &mut renderer,
    );
    ui.update(
        &[redraw()],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut messages,
    );
    assert_eq!(messages, vec![Message::NoFit(0)]);
    assert_eq!(anchors.visible(&Id::new("a")), None);
    assert!(
        anchors.surfaces().is_empty(),
        "a hidden source cannot reuse stale popup bounds"
    );
}

#[test]
fn one_pointer_click_dismisses_the_old_menu_and_opens_a_sibling_trigger() {
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let size = Size::new(300.0, 200.0);
    let mut ui = UserInterface::build(
        view(&anchors, true, true),
        size,
        user_interface::Cache::new(),
        &mut renderer,
    );
    let at = mouse::Cursor::Available(Point::new(90.0, 10.0));
    let mut messages = Vec::new();
    ui.update(
        &[Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        ))],
        at,
        &mut renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut messages,
    );
    assert_eq!(messages, vec![Message::Dismiss]);
    let mut ui = UserInterface::build(
        view(&anchors, false, true),
        size,
        ui.into_cache(),
        &mut renderer,
    );
    ui.update(
        &[Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        ))],
        at,
        &mut renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut messages,
    );
    assert_eq!(messages, vec![Message::Dismiss, Message::OpenB]);
}

#[test]
fn clicking_the_open_root_trigger_closes_once_without_reopening_on_release() {
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let size = Size::new(300.0, 200.0);
    let mut ui = UserInterface::build(
        view(&anchors, true, true),
        size,
        user_interface::Cache::new(),
        &mut renderer,
    );
    let at = mouse::Cursor::Available(Point::new(10.0, 10.0));
    let mut messages = Vec::new();
    let (_, statuses) = ui.update(
        &[Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        ))],
        at,
        &mut renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut messages,
    );
    assert_eq!(messages, vec![Message::Dismiss]);
    assert_eq!(statuses, vec![cosmic::iced::event::Status::Captured]);
    // The closed view now gives the trigger an opening callback. Its native
    // pressed state must not have been armed by the menu-closing press.
    let mut ui = UserInterface::build(
        view(&anchors, false, true),
        size,
        ui.into_cache(),
        &mut renderer,
    );
    ui.update(
        &[Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        ))],
        at,
        &mut renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut messages,
    );
    assert_eq!(
        messages,
        vec![Message::Dismiss],
        "one close, no opening callback on release"
    );
    assert!(!anchors.is_open(&Id::new("a")));
    assert!(anchors.surfaces().is_empty());
}

#[derive(Default)]
struct Focused(Vec<Id>);
impl Operation for Focused {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn focusable(&mut self, id: Option<&Id>, _: Rectangle, state: &mut dyn operation::Focusable) {
        if state.is_focused()
            && let Some(id) = id
        {
            self.0.push(id.clone());
        }
    }
}

#[test]
fn native_focus_moves_into_the_menu_and_returns_to_its_visible_trigger() {
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let size = Size::new(300.0, 200.0);
    let mut ui = UserInterface::build(
        view(&anchors, true, true),
        size,
        user_interface::Cache::new(),
        &mut renderer,
    );
    ui.update(
        &[redraw()],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut Vec::new(),
    );
    let mut focused = Focused::default();
    ui.operate(&renderer, &mut focused);
    assert_eq!(focused.0, vec![Id::new("menu-first")]);
    let enter = Event::Keyboard(keyboard::Event::KeyPressed {
        key: keyboard::Key::Named(keyboard::key::Named::Enter),
        modified_key: keyboard::Key::Named(keyboard::key::Named::Enter),
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Enter),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::empty(),
        text: None,
        repeat: false,
    });
    let mut actions = Vec::new();
    ui.update(
        &[enter],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut actions,
    );
    assert_eq!(
        actions,
        vec![Message::Action],
        "the focused opt-in target dispatches once after its native child declines Enter"
    );
    let mut ui = UserInterface::build(
        view(&anchors, false, true),
        size,
        ui.into_cache(),
        &mut renderer,
    );
    ui.update(
        &[redraw()],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut Vec::new(),
    );
    let mut focused = Focused::default();
    ui.operate(&renderer, &mut focused);
    assert_eq!(focused.0, vec![Id::new("a")]);
    assert!(!anchors.is_open(&Id::new("a")));
}

fn reported_component() -> crate::layout::Child<'static, Message> {
    use crate::layout::{InformationId, InformationLoss, LossKind, SourceId, forms};
    forms(vec![
        (
            Level::Preferred,
            child(|_| native::space().width(160).height(24).into()),
        ),
        (
            Level::Strong,
            child(|_| native::space().width(60).height(24).into()),
        ),
    ])
    .report_information(SourceId(41), 7, |level, _| {
        if level >= Level::Strong {
            vec![InformationLoss {
                identity: InformationId("label"),
                kind: LossKind::Omitted,
            }]
        } else {
            Vec::new()
        }
    })
}

fn reported_view(anchors: &Anchors, revision: u64, present: bool) -> Host<'static, Message> {
    let content: Element<'_, Message> = if present {
        let source = reported_component();
        let stages = crate::layout::stages(vec![
            Stage::new(Level::Preferred, source.clone()),
            Stage::new(Level::Strong, source),
        ]);
        anchors.reported_anchor("reported", crate::layout::SourceId(41), revision, stages)
    } else {
        native::space().width(60).height(24).into()
    };
    host(content).anchors(anchors.clone())
}

#[test]
fn accepted_source_reports_follow_form_revision_and_removal() {
    use crate::layout::{InformationId, InformationLoss, LossKind};
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let id = Id::new("reported");
    // Disposable queries produce facts only in their returned plan, never in
    // the live anchor registry.
    reported_component()
        .query_at(Level::Strong, &renderer, Size::new(80.0, 100.0))
        .unwrap();
    assert!(anchors.information(&id).is_none());

    let mut ui = UserInterface::build(
        reported_view(&anchors, 7, true),
        Size::new(200.0, 100.0),
        user_interface::Cache::new(),
        &mut renderer,
    );
    let mut messages = Vec::new();
    let mut clipboard = cosmic::iced::advanced::clipboard::Null;
    ui.update(
        &[redraw()],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut clipboard,
        &mut messages,
    );
    let report = anchors.information(&id).expect("accepted preferred report");
    assert_eq!(report.revision, 7);
    assert_eq!(report.level, Level::Preferred);
    assert!(report.losses.is_empty());
    reported_component()
        .query_at(Level::Strong, &renderer, Size::new(80.0, 100.0))
        .unwrap();
    assert_eq!(
        anchors.information(&id).unwrap(),
        report,
        "trial queries cannot replace accepted facts"
    );

    let mut ui = ui.relayout(Size::new(80.0, 100.0), &mut renderer);
    ui.update(
        &[redraw()],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut clipboard,
        &mut messages,
    );
    let report = anchors.information(&id).expect("accepted compact report");
    assert_eq!(report.level, Level::Strong);
    assert_eq!(
        report.losses,
        vec![InformationLoss {
            identity: InformationId("label"),
            kind: LossKind::Omitted
        }]
    );

    let mut ui = UserInterface::build(
        reported_view(&anchors, 8, true),
        Size::new(80.0, 100.0),
        ui.into_cache(),
        &mut renderer,
    );
    ui.update(
        &[redraw()],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut clipboard,
        &mut messages,
    );
    assert!(
        anchors.information(&id).is_none(),
        "mismatched revision must discard the old report"
    );

    // Restore a live report first, so disappearance tests stale-state clearing
    // independently of the revision-mismatch case.
    let mut ui = UserInterface::build(
        reported_view(&anchors, 7, true),
        Size::new(80.0, 100.0),
        ui.into_cache(),
        &mut renderer,
    );
    ui.update(
        &[redraw()],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut clipboard,
        &mut messages,
    );
    assert!(anchors.information(&id).is_some());

    let mut ui = UserInterface::build(
        reported_view(&anchors, 7, false),
        Size::new(80.0, 100.0),
        ui.into_cache(),
        &mut renderer,
    );
    ui.update(
        &[redraw()],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut clipboard,
        &mut messages,
    );
    assert!(
        anchors.information(&id).is_none(),
        "a removed source cannot retain facts"
    );
    assert!(anchors.visible(&id).is_none());
    assert!(messages.is_empty());
}

#[test]
fn ambiguous_reports_for_one_identity_are_not_published() {
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let duplicate = crate::layout::row_child(vec![reported_component(), reported_component()], 0.0);
    let content = crate::layout::stages(vec![Stage::new(Level::Preferred, duplicate)]);
    let source = anchors.reported_anchor("duplicate", crate::layout::SourceId(41), 7, content);
    let mut ui = UserInterface::build(
        host(source).anchors(anchors.clone()),
        Size::new(400.0, 100.0),
        user_interface::Cache::new(),
        &mut renderer,
    );
    ui.update(
        &[redraw()],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut Vec::new(),
    );
    let id = Id::new("duplicate");
    assert!(anchors.visible(&id).is_some());
    assert!(anchors.information(&id).is_none());
}
