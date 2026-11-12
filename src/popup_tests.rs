// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::layout::{Level, child};
use cosmic::iced::advanced::renderer::{Headless, Renderer as _};
use cosmic::iced::{Color, Font, Pixels, Point};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Message {
    Input(String),
    Dismiss,
    Close(usize),
    Action,
    NoFit(usize),
}

fn renderer() -> Renderer {
    cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
        Font::default(),
        Pixels(14.0),
        Some("tiny-skia"),
    ))
    .expect("headless tiny-skia renderer")
}
fn key(named: keyboard::key::Named) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: keyboard::Key::Named(named),
        modified_key: keyboard::Key::Named(named),
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Enter),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::empty(),
        text: None,
        repeat: false,
    })
}
fn character(value: &'static str) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: keyboard::Key::Character(value.into()),
        modified_key: keyboard::Key::Character(value.into()),
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyA),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::empty(),
        text: Some(value.into()),
        repeat: false,
    })
}
fn base() -> Element<'static, Message> {
    widget::space()
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
fn root_node(
    host: &mut Host<'_, Message>,
    tree: &mut Tree,
    renderer: &Renderer,
    size: Size,
) -> layout::Node {
    host.diff(tree);
    host.layout(tree, renderer, &layout::Limits::new(Size::ZERO, size))
}
fn dispatch(
    host: &mut Host<'_, Message>,
    tree: &mut Tree,
    renderer: &Renderer,
    size: Size,
    event: &Event,
    cursor: mouse::Cursor,
) -> (Vec<Message>, bool) {
    let node = root_node(host, tree, renderer, size);
    let viewport = Rectangle::with_size(size);
    let mut messages = Vec::new();
    let mut shell = Shell::new(&mut messages);
    let mut blocked = false;
    if let Some(mut overlay) =
        host.overlay(tree, Layout::new(&node), renderer, &viewport, Vector::ZERO)
    {
        let overlay_node = overlay.as_overlay_mut().layout(renderer, size);
        blocked =
            overlay
                .as_overlay()
                .mouse_interaction(Layout::new(&overlay_node), cursor, renderer)
                != mouse::Interaction::None;
        overlay.as_overlay_mut().update(
            event,
            Layout::new(&overlay_node),
            cursor,
            renderer,
            &mut cosmic::iced::advanced::clipboard::Null,
            &mut shell,
        );
    }
    if !shell.is_event_captured() {
        host.update(
            tree,
            event,
            Layout::new(&node),
            if blocked {
                mouse::Cursor::Unavailable
            } else {
                cursor
            },
            renderer,
            &mut cosmic::iced::advanced::clipboard::Null,
            &mut shell,
            &viewport,
        );
    }
    let captured = shell.is_event_captured();
    drop(shell);
    (messages, captured)
}
fn focus(host: &mut Host<'_, Message>, tree: &mut Tree, renderer: &Renderer, size: Size, id: Id) {
    let node = root_node(host, tree, renderer, size);
    let viewport = Rectangle::with_size(size);
    let mut overlay = host
        .overlay(tree, Layout::new(&node), renderer, &viewport, Vector::ZERO)
        .unwrap();
    let overlay_node = overlay.as_overlay_mut().layout(renderer, size);
    overlay.as_overlay_mut().operate(
        Layout::new(&overlay_node),
        renderer,
        &mut operation::focusable::focus::<()>(id),
    );
}
fn editor(value: Rc<RefCell<String>>) -> Panel<'static, Message> {
    Panel::new(
        Attachment::below(Rectangle::new(
            Point::new(20.0, 10.0),
            Size::new(100.0, 20.0),
        )),
        vec![Stage::new(
            Level::Preferred,
            child(move |_| {
                let current = value.borrow().clone();
                let value = value.clone();
                widget::text_input("Search", current)
                    .id(Id::new("popup-editor"))
                    .width(200)
                    .on_input(move |next| {
                        *value.borrow_mut() = next.clone();
                        Message::Input(next)
                    })
                    .into()
            }),
        )],
    )
    .key(1)
    .focus_targets(vec![
        FocusTarget::new("popup-editor").activate(Message::Action),
    ])
}

#[test]
fn native_editor_edits_and_its_focus_survive_popup_resize() {
    let renderer = renderer();
    let value = Rc::new(RefCell::new(String::new()));
    let mut host = host(base())
        .chain(vec![editor(value.clone())])
        .on_dismiss(Message::Dismiss);
    let mut tree = Tree::new(&host as &dyn Widget<Message, Theme, Renderer>);
    let large = Size::new(500.0, 300.0);
    focus(
        &mut host,
        &mut tree,
        &renderer,
        large,
        Id::new("popup-editor"),
    );
    assert_eq!(
        dispatch(
            &mut host,
            &mut tree,
            &renderer,
            large,
            &character("a"),
            mouse::Cursor::Unavailable
        )
        .0,
        vec![Message::Input("a".into())]
    );
    let small = Size::new(260.0, 160.0);
    assert_eq!(
        dispatch(
            &mut host,
            &mut tree,
            &renderer,
            small,
            &character("b"),
            mouse::Cursor::Unavailable
        )
        .0,
        vec![Message::Input("ab".into())]
    );
    // A native editor gets cursor keys first; the host must not turn this into
    // menu navigation and must use its native focus identity after handling.
    let (messages, captured) = dispatch(
        &mut host,
        &mut tree,
        &renderer,
        small,
        &key(keyboard::key::Named::ArrowLeft),
        mouse::Cursor::Unavailable,
    );
    assert!(captured);
    assert!(messages.is_empty());
    assert_eq!(
        dispatch(
            &mut host,
            &mut tree,
            &renderer,
            small,
            &character("c"),
            mouse::Cursor::Unavailable
        )
        .0,
        vec![Message::Input("acb".into())]
    );
    assert_eq!(&*value.borrow(), "acb");
}

#[test]
fn replacing_a_panel_key_resets_native_focus_but_same_key_retains_it() {
    let renderer = renderer();
    let size = Size::new(500.0, 300.0);
    let mut host = host(base()).chain(vec![editor(Rc::new(RefCell::new(String::new())))]);
    let mut tree = Tree::new(&host as &dyn Widget<Message, Theme, Renderer>);
    focus(
        &mut host,
        &mut tree,
        &renderer,
        size,
        Id::new("popup-editor"),
    );
    assert!(
        !dispatch(
            &mut host,
            &mut tree,
            &renderer,
            size,
            &character("a"),
            mouse::Cursor::Unavailable
        )
        .0
        .is_empty()
    );
    host.panels[0].key = 2;
    assert!(
        dispatch(
            &mut host,
            &mut tree,
            &renderer,
            size,
            &character("b"),
            mouse::Cursor::Unavailable
        )
        .0
        .is_empty()
    );
    assert_eq!(tree.state.downcast_ref::<State>().focused, vec![None]);
}

fn empty_panel(anchor: Rectangle) -> Panel<'static, Message> {
    Panel::new(
        Attachment::below(anchor),
        vec![Stage::new(
            Level::Preferred,
            child(|_| widget::space().width(100).height(40).into()),
        )],
    )
}

#[test]
fn submenu_anchors_follow_committed_parent_and_escape_closes_one_level() {
    let renderer = renderer();
    let root = empty_panel(Rectangle::new(
        Point::new(250.0, 120.0),
        Size::new(20.0, 10.0),
    ));
    let submenu = Panel::new(
        Attachment::beside(Rectangle::new(Point::new(0.0, 0.0), Size::new(100.0, 20.0))),
        vec![Stage::new(
            Level::Preferred,
            child(|_| widget::space().width(80).height(30).into()),
        )],
    )
    .relative_to_previous()
    .key(2);
    let mut host = host(base())
        .chain(vec![root, submenu])
        .on_dismiss(Message::Dismiss)
        .on_close(Message::Close);
    let mut tree = Tree::new(&host as &dyn Widget<Message, Theme, Renderer>);
    let (messages, captured) = dispatch(
        &mut host,
        &mut tree,
        &renderer,
        Size::new(300.0, 200.0),
        &key(keyboard::key::Named::Escape),
        mouse::Cursor::Unavailable,
    );
    assert!(captured);
    assert_eq!(messages, vec![Message::Close(1)]);
    let parent = host.panels[0].selection().as_ref().unwrap().bounds;
    let submenu = host.panels[1].selection().as_ref().unwrap().bounds;
    assert_eq!(
        submenu.x + submenu.width,
        parent.x,
        "submenu flips left using real parent geometry"
    );
    assert_eq!(submenu.y, parent.y);
}

#[test]
fn outside_press_dismisses_and_no_fit_is_reported_without_implicit_clipping() {
    let renderer = renderer();
    let mut host = host(base())
        .chain(vec![empty_panel(Rectangle::new(
            Point::new(20.0, 10.0),
            Size::new(20.0, 20.0),
        ))])
        .on_dismiss(Message::Dismiss)
        .on_no_fit(Message::NoFit);
    let mut tree = Tree::new(&host as &dyn Widget<Message, Theme, Renderer>);
    let click = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    let (messages, captured) = dispatch(
        &mut host,
        &mut tree,
        &renderer,
        Size::new(300.0, 200.0),
        &click,
        mouse::Cursor::Available(Point::new(280.0, 180.0)),
    );
    assert_eq!(messages, vec![Message::Dismiss]);
    assert!(captured);
    let (messages, _) = dispatch(
        &mut host,
        &mut tree,
        &renderer,
        Size::new(60.0, 30.0),
        &Event::Window(cosmic::iced::window::Event::RedrawRequested(
            std::time::Instant::now(),
        )),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(messages, vec![Message::NoFit(0)]);
    assert!(host.panels[0].selection().is_err());
}

#[test]
fn disabled_native_child_can_show_its_nested_explanation() {
    let renderer = renderer();
    let panel = Panel::new(
        Attachment::below(Rectangle::new(Point::ORIGIN, Size::new(20.0, 20.0))),
        vec![Stage::new(
            Level::Preferred,
            child(|_| {
                widget::tooltip(
                    widget::button::text("Unavailable"),
                    widget::text("Explains why"),
                    widget::tooltip::Position::Bottom,
                )
                .into()
            }),
        )],
    );
    let mut host = host(base()).chain(vec![panel]);
    let mut tree = Tree::new(&host as &dyn Widget<Message, Theme, Renderer>);
    let size = Size::new(400.0, 300.0);
    let point = Point::new(20.0, 30.0);
    dispatch(
        &mut host,
        &mut tree,
        &renderer,
        size,
        &Event::Mouse(mouse::Event::CursorMoved { position: point }),
        mouse::Cursor::Available(point),
    );
    let node = root_node(&mut host, &mut tree, &renderer, size);
    let viewport = Rectangle::with_size(size);
    let mut overlay = host
        .overlay(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &viewport,
            Vector::ZERO,
        )
        .unwrap();
    let overlay_node = overlay.as_overlay_mut().layout(&renderer, size);
    assert!(
        overlay
            .as_overlay_mut()
            .overlay(Layout::new(&overlay_node), &renderer)
            .is_some(),
        "disabled controls retain nested hover explanations"
    );
}

#[test]
fn unrelated_base_hover_is_available_and_its_overlay_paints_after_the_menu() {
    let mut renderer = renderer();
    let base: Element<'_, Message> = widget::tooltip(
        widget::space().width(80).height(30),
        Element::new(Paint {
            size: Size::new(80.0, 40.0),
            color: Color::from_rgb(0.0, 1.0, 0.0),
        }),
        widget::tooltip::Position::Bottom,
    )
    .padding(0.0)
    .into();
    let panel = Panel::new(
        Attachment::below(Rectangle::new(Point::new(0.0, 20.0), Size::new(20.0, 20.0))),
        vec![Stage::new(
            Level::Preferred,
            child(|_| {
                Element::new(Paint {
                    size: Size::new(100.0, 40.0),
                    color: Color::from_rgb(1.0, 0.0, 0.0),
                })
            }),
        )],
    );
    let mut host = host(base).chain(vec![panel]);
    let mut tree = Tree::new(&host as &dyn Widget<Message, Theme, Renderer>);
    let size = Size::new(400.0, 300.0);
    let at = Point::new(10.0, 10.0);
    dispatch(
        &mut host,
        &mut tree,
        &renderer,
        size,
        &Event::Mouse(mouse::Event::CursorMoved { position: at }),
        mouse::Cursor::Available(at),
    );
    let node = root_node(&mut host, &mut tree, &renderer, size);
    let viewport = Rectangle::with_size(size);
    let mut overlay = host
        .overlay(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &viewport,
            Vector::ZERO,
        )
        .unwrap();
    let overlay_node = overlay.as_overlay_mut().layout(&renderer, size);
    assert_eq!(
        overlay_node.children().len(),
        2,
        "both chain and unrelated tooltip remain exposed"
    );
    assert_eq!(
        overlay_node.children()[0].size(),
        size,
        "chain is first paint child at index 0.5"
    );
    // Tooltip::overlay returns a Group even for one bubble. Both outer
    // children therefore have viewport-sized roots; inspect the bubble inside
    // the second group and verify the actual overlapping paint below.
    let bubble = &overlay_node.children()[1].children()[0];
    assert_eq!(bubble.size(), Size::new(80.0, 40.0));
    assert!(bubble.bounds().contains(Point::new(10.0, 50.0)));
    assert!(!bubble.bounds().contains(Point::new(90.0, 50.0)));
    assert_eq!(
        overlay.as_overlay().mouse_interaction(
            Layout::new(&overlay_node),
            mouse::Cursor::Available(at),
            &renderer
        ),
        mouse::Interaction::None,
        "full-window overlay root does not blanket-block base hover"
    );
    let theme = Theme::light();
    overlay.as_overlay().draw(
        &mut renderer,
        &theme,
        &renderer::Style {
            icon_color: Color::BLACK,
            text_color: Color::BLACK,
            scale_factor: 1.0,
        },
        Layout::new(&overlay_node),
        mouse::Cursor::Available(at),
    );
    drop(overlay);
    let pixels = renderer.screenshot(
        Size::new(size.width as u32, size.height as u32),
        1.0,
        Color::WHITE,
    );
    let pixel = |x: usize, y: usize| &pixels[(y * size.width as usize + x) * 4..][..4];
    assert_eq!(
        pixel(10, 50),
        &[0, 255, 0, 255],
        "tooltip wins where its green content overlaps the red menu"
    );
    assert_eq!(
        pixel(90, 50),
        &[255, 0, 0, 255],
        "the menu remains visible beside the tooltip"
    );
}

struct Paint {
    size: Size,
    color: Color,
}
impl Widget<Message, Theme, Renderer> for Paint {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }
    fn layout(&mut self, _: &mut Tree, _: &Renderer, _: &layout::Limits) -> layout::Node {
        layout::Node::new(self.size)
    }
    fn draw(
        &self,
        _: &Tree,
        renderer: &mut Renderer,
        _: &Theme,
        _: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
        renderer.fill_quad(
            renderer::Quad {
                bounds: layout.bounds(),
                ..Default::default()
            },
            self.color,
        );
    }
}
