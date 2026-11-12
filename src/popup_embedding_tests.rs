// SPDX-License-Identifier: GPL-3.0-only
//! Arbitrary named native controls keep their state when physical ownership changes.
use super::*;
use crate::layout::{Alignment, AnchorCandidate, Level, Side, child};
use cosmic::iced::advanced::renderer::Headless;
use cosmic::iced::{Font, Pixels};

#[derive(Clone, Debug, PartialEq)]
enum Message {
    Input(String),
    NoFit(usize),
}
fn editor() -> Element<'static, Message> {
    widget::text_input("Editor", "draft")
        .id(Id::new("embedded-editor"))
        .width(140)
        .on_input(Message::Input)
        .into()
}
fn root(anchors: &Anchors, embedded: bool) -> Element<'static, Message> {
    let folder = anchors.anchor("embedded-folder", widget::space().width(160).height(28));
    let mut children = vec![folder];
    if embedded {
        children.push(editor());
    }
    widget::column(children).width(180).into()
}
fn fixture(anchors: &Anchors) -> Host<'static, Message> {
    let trigger = anchors.trigger("embedded-source", widget::space().width(40).height(20));
    let base = widget::column(vec![widget::space().height(Length::Fill).into(), trigger])
        .height(Length::Fill);
    let normal = anchors.clone();
    let embedded = anchors.clone();
    let root = Panel::new(
        Attachment::new(
            Rectangle::default(),
            vec![AnchorCandidate::new(Side::Above, Alignment::StartBounded)],
        ),
        vec![Stage::new(
            Level::Preferred,
            child(move |_| root(&normal, false)),
        )],
    )
    .source("embedded-source")
    .key(1)
    .embedded_compositions(vec![vec![Stage::new(
        Level::Preferred,
        child(move |_| root(&embedded, true)),
    )]]);
    let editor = Panel::new(
        Attachment::new(
            Rectangle::default(),
            vec![
                AnchorCandidate::new(Side::Right, Alignment::StartBounded),
                AnchorCandidate::new(Side::Left, Alignment::StartBounded),
            ],
        ),
        vec![Stage::new(Level::Preferred, child(|_| editor()))],
    )
    .source("embedded-folder")
    .beside_previous_surface()
    .key(2)
    .focus_targets(vec![FocusTarget::new("embedded-editor")])
    .focus_first();
    host(base)
        .anchors(anchors.clone())
        .chain(vec![root, editor])
        .on_no_fit(Message::NoFit)
}
fn layout_root(
    host: &mut Host<'_, Message>,
    tree: &mut Tree,
    renderer: &Renderer,
    size: Size,
) -> layout::Node {
    host.layout(tree, renderer, &layout::Limits::new(Size::ZERO, size))
}
#[test]
fn native_editor_selection_survives_beside_inline_beside_without_view_rebuild() {
    let renderer = cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
        Font::default(),
        Pixels(14.0),
        Some("tiny-skia"),
    ))
    .expect("native renderer");
    let anchors = Anchors::default();
    let mut host = fixture(&anchors);
    let mut tree = Tree::new(&host as &dyn Widget<Message, Theme, Renderer>);
    host.diff(&mut tree);
    let wide = Size::new(600.0, 240.0);
    let narrow = Size::new(233.0, 240.0);
    let node = layout_root(&mut host, &mut tree, &renderer, wide);
    {
        let mut overlay = host
            .overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &Rectangle::with_size(wide),
                Vector::ZERO,
            )
            .expect("chain");
        let node = overlay.as_overlay_mut().layout(&renderer, wide);
        overlay.as_overlay_mut().operate(
            Layout::new(&node),
            &renderer,
            &mut operation::text_input::select_all::<()>(Id::new("embedded-editor")),
        );
    }
    assert_eq!(anchors.surfaces().len(), 2);
    for size in [narrow, wide] {
        let node = layout_root(&mut host, &mut tree, &renderer, size);
        let mut overlay = host
            .overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &Rectangle::with_size(size),
                Vector::ZERO,
            )
            .expect("chain");
        let node = overlay.as_overlay_mut().layout(&renderer, size);
        let mut probe = FocusProbe::default();
        overlay
            .as_overlay_mut()
            .operate(Layout::new(&node), &renderer, &mut probe);
        assert_eq!(
            probe.focused,
            Some(Id::new("embedded-editor")),
            "focus retained across physical surface changes"
        );
    }
    assert_eq!(anchors.surfaces().len(), 2);
    let node = layout_root(&mut host, &mut tree, &renderer, wide);
    let mut overlay = host
        .overlay(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &Rectangle::with_size(wide),
            Vector::ZERO,
        )
        .expect("chain");
    let node = overlay.as_overlay_mut().layout(&renderer, wide);
    let event = Event::Keyboard(keyboard::Event::KeyPressed {
        key: keyboard::Key::Character("x".into()),
        modified_key: keyboard::Key::Character("x".into()),
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyX),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::empty(),
        text: Some("x".into()),
        repeat: false,
    });
    let mut messages = Vec::new();
    let mut shell = Shell::new(&mut messages);
    overlay.as_overlay_mut().update(
        &event,
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut shell,
    );
    assert_eq!(
        messages,
        [Message::Input("x".into())],
        "retained native selection replaces the whole draft after reparenting"
    );
}
