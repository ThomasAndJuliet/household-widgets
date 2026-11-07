// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::menu::Row;
use std::rc::Rc;

fn words() -> TextMenuWords {
    TextMenuWords {
        cut: "Cut".into(),
        copy: "Copy".into(),
        paste: "Paste".into(),
        select_all: "Select all".into(),
        no_selection: "No selection".into(),
        hidden: "Hidden".into(),
        read_only: "Read only".into(),
        clipboard_empty: "Clipboard empty".into(),
        empty_content: "Empty".into(),
    }
}
fn icon(name: &'static str) -> cosmic::widget::icon::Handle {
    cosmic::widget::icon::from_name(name).handle()
}
fn context() -> TextMenuContext {
    TextMenuContext {
        kind: TextKind::Editable,
        secure: false,
        has_selection: true,
        has_text: true,
        clipboard_has_text: true,
    }
}
fn rows(context: TextMenuContext) -> Vec<(String, Option<String>)> {
    let (menu, _) = resolved_menu(context, &words(), Menu::<()>::new(), icon);
    menu.rows()
        .iter()
        .filter_map(|row| match row {
            Row::Item(item) => Some((item.label.clone(), item.disabled_reason.clone())),
            _ => None,
        })
        .collect()
}
#[test]
fn capabilities_and_reason_priority() {
    let mut c = context();
    assert_eq!(
        rows(c).iter().map(|r| r.1.as_deref()).collect::<Vec<_>>(),
        vec![None, None, None, None]
    );
    c.has_selection = false;
    c.has_text = false;
    c.clipboard_has_text = false;
    assert_eq!(
        rows(c).iter().map(|r| r.1.as_deref()).collect::<Vec<_>>(),
        vec![
            Some("No selection"),
            Some("No selection"),
            Some("Clipboard empty"),
            Some("Empty")
        ]
    );
    c.secure = true;
    assert_eq!(rows(c)[0].1.as_deref(), Some("Hidden"));
    assert_eq!(rows(c)[1].1.as_deref(), Some("Hidden"));
    c.kind = TextKind::ReadOnlyInput;
    assert_eq!(
        rows(c).iter().map(|r| r.1.as_deref()).collect::<Vec<_>>(),
        vec![
            Some("Read only"),
            Some("Hidden"),
            Some("Read only"),
            Some("Empty")
        ]
    );
    c.kind = TextKind::Selectable;
    c.secure = false;
    assert_eq!(
        rows(c),
        vec![
            ("Copy".into(), Some("No selection".into())),
            ("Select all".into(), Some("Empty".into()))
        ]
    );
}
#[test]
fn standard_rows_have_native_shortcuts_and_expected_icons() {
    let (menu, _) = resolved_menu(context(), &words(), Menu::<()>::new(), icon);
    let items: Vec<_> = menu
        .rows()
        .iter()
        .filter_map(|row| match row {
            Row::Item(item) => Some(item),
            _ => None,
        })
        .collect();
    assert_eq!(
        items
            .iter()
            .map(|item| item.icon.is_some())
            .collect::<Vec<_>>(),
        vec![true, true, true, false]
    );
    assert_eq!(
        items
            .iter()
            .map(|item| item.shortcut.as_ref().unwrap().key.clone())
            .collect::<Vec<_>>(),
        ["x", "c", "v", "a"]
            .into_iter()
            .map(|letter| Key::Character(letter.into()))
            .collect::<Vec<_>>()
    );
    assert!(
        items
            .iter()
            .all(|item| item.shortcut.as_ref().unwrap().modifiers == vec![Modifier::Ctrl])
    );
}
#[test]
fn custom_snapshot_is_local_and_selection_sets_group_order() {
    let token = Rc::new(7);
    let custom = Menu::new().item(MenuItem {
        label: "Send".into(),
        message: token.clone(),
        icon: None,
        shortcut: None,
        mark: None,
        disabled_reason: None,
        destructive: false,
    });
    let (menu, snapshot) = resolved_menu(context(), &words(), custom.clone(), icon);
    assert_eq!(menu.outline()[0].split('\t').next(), Some("Cut"));
    assert!(Rc::ptr_eq(&snapshot[0], &token));
    let mut c = context();
    c.has_selection = false;
    let (menu, snapshot) = resolved_menu(c, &words(), custom, icon);
    assert_eq!(menu.outline()[0], "Send");
    assert!(Rc::ptr_eq(&snapshot[0], &token));
}
#[test]
fn real_native_targets_supply_the_policy_context() {
    fn context_of<W: Widget<(), Theme, Renderer> + TextMenuTarget<()>>(
        target: &W,
    ) -> TextMenuContext {
        let tree = Tree::new(target as &dyn Widget<(), Theme, Renderer>);
        target.text_menu_context(&tree)
    }
    let input = cosmic::widget::text_input("", "value").on_input(|_| ());
    let input_context = context_of(&input);
    assert_eq!(input_context.kind, TextKind::Editable);
    assert_eq!(rows(input_context).len(), 4);
    let fixed = cosmic::widget::text_input::<()>("", "value");
    assert_eq!(context_of(&fixed).kind, TextKind::ReadOnlyInput);
    let content = cosmic::widget::text_editor::Content::<Renderer>::new();
    let editor = cosmic::widget::text_editor::text_editor(&content).on_action(|_| ());
    assert_eq!(context_of(&editor).kind, TextKind::Editable);
    let text = cosmic::widget::text("value").selectable();
    let text_context = context_of(&text);
    assert_eq!(text_context.kind, TextKind::Selectable);
    assert_eq!(rows(text_context).len(), 2);
}

#[test]
fn real_targets_open_the_household_overlay_on_right_press() {
    use cosmic::iced::advanced::renderer::Headless;
    use cosmic::iced::{Font, Pixels, Point};
    fn body(text: String) -> Element<'static, TextMenuCommand> {
        cosmic::widget::text(text).into()
    }
    fn exercise<W: Widget<(), Theme, Renderer> + TextMenuTarget<()>>(
        target: W,
        renderer: &Renderer,
    ) {
        let mut menu =
            text_menu(target, &words(), MenuStyle { icon, body }).window_id(window::Id::NONE);
        let mut tree = Tree::new(&menu as &dyn Widget<(), Theme, Renderer>);
        let viewport = Rectangle::with_size(Size::new(400.0, 300.0));
        let node = menu.layout(
            &mut tree,
            renderer,
            &layout::Limits::new(Size::ZERO, viewport.size()),
        );
        let cursor = mouse::Cursor::Available(Point::new(8.0, 8.0));
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        menu.update(
            &mut tree,
            &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
            Layout::new(&node),
            cursor,
            renderer,
            &mut cosmic::iced::advanced::clipboard::Null,
            &mut shell,
            &viewport,
        );
        assert!(
            menu.overlay(
                &mut tree,
                Layout::new(&node),
                renderer,
                &viewport,
                Vector::ZERO
            )
            .is_some()
        );
    }
    let renderer = cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
        Font::default(),
        Pixels(14.0),
        Some("tiny-skia"),
    ))
    .expect("a headless renderer");
    exercise(
        cosmic::widget::text_input("", "value").on_input(|_| ()),
        &renderer,
    );
    let content = cosmic::widget::text_editor::Content::<Renderer>::with_text("value");
    exercise(
        cosmic::widget::text_editor::text_editor(&content).on_action(|_| ()),
        &renderer,
    );
    exercise(cosmic::widget::text("value").selectable(), &renderer);
}

#[test]
fn size_hint_preserves_the_targets_distinct_layout_hint() {
    struct HintTarget;
    impl Widget<(), Theme, Renderer> for HintTarget {
        fn size(&self) -> Size<Length> {
            Size::new(Length::Shrink, Length::Shrink)
        }
        fn size_hint(&self) -> Size<Length> {
            Size::new(Length::Fill, Length::Shrink)
        }
        fn layout(&mut self, _: &mut Tree, _: &Renderer, _: &layout::Limits) -> layout::Node {
            layout::Node::new(Size::ZERO)
        }
        fn draw(
            &self,
            _: &Tree,
            _: &mut Renderer,
            _: &Theme,
            _: &renderer::Style,
            _: Layout<'_>,
            _: mouse::Cursor,
            _: &Rectangle,
        ) {
        }
    }
    impl TextMenuTarget<()> for HintTarget {
        fn use_external_text_menu(&mut self) {}
        fn take_text_menu_request(&self, _: &mut Tree) -> Option<cosmic::iced::Point> {
            None
        }
        fn text_menu_context(&self, _: &Tree) -> TextMenuContext {
            context()
        }
        fn execute_text_menu_action(
            &mut self,
            _: &mut Tree,
            _: TextAction,
            _: &mut dyn Clipboard,
            _: &mut Shell<'_, ()>,
        ) {
        }
    }
    let menu = text_menu(HintTarget, &words(), style());
    assert_eq!(
        Widget::<(), Theme, Renderer>::size(&menu).width,
        Length::Shrink
    );
    assert_eq!(
        Widget::<(), Theme, Renderer>::size_hint(&menu).width,
        Length::Fill
    );
}

#[test]
fn named_input_keeps_its_selection_when_tree_state_is_reconciled() {
    use cosmic::iced::advanced::widget::{Id, tree::NAMED};
    let id = Id::new("household-text-menu-named-input");
    let make = || {
        text_menu(
            cosmic::widget::text_input("", "chosen")
                .id(id.clone())
                .on_input(Message::Input),
            &words(),
            style(),
        )
    };
    let mut original = make();
    let mut old_tree = tree(&original);
    original.diff(&mut old_tree);
    assert_eq!(old_tree.id, None, "only the target owns the name");
    assert_eq!(old_tree.children[0].id, Some(id.clone()));
    old_tree.children[0]
        .state
        .downcast_mut::<cosmic::widget::text_input::State>()
        .select_all();
    assert!(
        original
            .content
            .text_menu_context(&old_tree.children[0])
            .has_selection
    );
    let named = old_tree.take_all_named();
    assert_eq!(
        named.len(),
        1,
        "the wrapper must not duplicate the child's name"
    );
    NAMED.with(|map| map.borrow_mut().extend(named));
    let mut restored = make();
    let mut new_tree = tree(&restored);
    restored.diff(&mut new_tree);
    assert!(
        restored
            .content
            .text_menu_context(&new_tree.children[0])
            .has_selection,
        "selection follows the named target into the new tree"
    );
    NAMED.with(|map| map.borrow_mut().clear());
}

#[test]
fn setting_an_id_on_the_wrapper_assigns_it_to_its_target() {
    use cosmic::iced::advanced::widget::Id;
    let id = Id::new("household-text-menu-set-id");
    let mut menu = text_menu(
        cosmic::widget::text_input("", "value").on_input(Message::Input),
        &words(),
        style(),
    );
    Widget::<Message, Theme, Renderer>::set_id(&mut menu, id.clone());
    let state = tree(&menu);
    assert_eq!(state.id, None);
    assert_eq!(state.children[0].id, Some(id));
}

#[cfg(feature = "a11y")]
#[test]
fn selectable_text_accessibility_nodes_pass_through_the_wrapper() {
    let renderer = renderer();
    let target = cosmic::widget::text("Accessible paragraph").selectable();
    let mut menu = text_menu::<_, Message>(target, &words(), style());
    let mut state = tree(&menu);
    let node = menu.layout(
        &mut state,
        &renderer,
        &layout::Limits::new(Size::ZERO, VIEWPORT.size()),
    );
    let layout = Layout::new(&node);
    let cursor = mouse::Cursor::Unavailable;
    let expected = Widget::<Message, Theme, Renderer>::a11y_nodes(
        &menu.content,
        layout,
        &state.children[0],
        cursor,
    );
    let actual = menu.a11y_nodes(layout, &state, cursor);
    assert!(
        !expected.root().is_empty(),
        "the native text contributes an accessible node"
    );
    assert_eq!(actual.root(), expected.root());
    assert_eq!(actual.children(), expected.children());
}

#[derive(Clone, Debug)]
enum Message {
    Input(String),
    Paste(String),
    Editor(cosmic::widget::text_editor::Action),
    Custom(Rc<str>),
}

#[derive(Default)]
struct Board(Option<String>);
impl Clipboard for Board {
    fn read(&self, _: cosmic::iced::advanced::clipboard::Kind) -> Option<String> {
        self.0.clone()
    }
    fn write(&mut self, _: cosmic::iced::advanced::clipboard::Kind, text: String) {
        self.0 = Some(text);
    }
}
fn body(text: String) -> Element<'static, TextMenuCommand> {
    cosmic::widget::text(text).into()
}
fn style() -> MenuStyle<TextMenuCommand> {
    MenuStyle { icon, body }
}
fn renderer() -> Renderer {
    use cosmic::iced::advanced::renderer::Headless;
    cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
        cosmic::iced::Font::default(),
        cosmic::iced::Pixels(14.0),
        Some("tiny-skia"),
    ))
    .expect("a headless renderer")
}
const VIEWPORT: Rectangle = Rectangle {
    x: 0.0,
    y: 0.0,
    width: 520.0,
    height: 300.0,
};
const POINT: cosmic::iced::Point = cosmic::iced::Point::new(30.0, 12.0);

fn source_event<W: Widget<Message, Theme, Renderer> + TextMenuTarget<Message>>(
    menu: &mut TextMenu<W, Message>,
    tree: &mut Tree,
    renderer: &Renderer,
    board: &mut Board,
    event: Event,
    at: cosmic::iced::Point,
) -> Vec<Message> {
    // UserInterface::build reconciles the widget before layout and update.
    // The native input populates its tracked value during that reconciliation.
    menu.diff(tree);
    let node = menu.layout(
        tree,
        renderer,
        &layout::Limits::new(Size::ZERO, VIEWPORT.size()),
    );
    let mut messages = Vec::new();
    let mut shell = Shell::new(&mut messages);
    menu.update(
        tree,
        &event,
        Layout::new(&node),
        mouse::Cursor::Available(at),
        renderer,
        board,
        &mut shell,
        &VIEWPORT,
    );
    messages
}
fn press<W: Widget<Message, Theme, Renderer> + TextMenuTarget<Message>>(
    menu: &mut TextMenu<W, Message>,
    tree: &mut Tree,
    renderer: &Renderer,
    board: &mut Board,
) {
    let messages = source_event(
        menu,
        tree,
        renderer,
        board,
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
        POINT,
    );
    assert!(messages.is_empty());
}
fn choose<W: Widget<Message, Theme, Renderer> + TextMenuTarget<Message>>(
    menu: &mut TextMenu<W, Message>,
    tree: &mut Tree,
    renderer: &Renderer,
    board: &mut Board,
    row: usize,
) {
    let node = menu.layout(
        tree,
        renderer,
        &layout::Limits::new(Size::ZERO, VIEWPORT.size()),
    );
    let mut overlay = menu
        .overlay(tree, Layout::new(&node), renderer, &VIEWPORT, Vector::ZERO)
        .expect("open menu");
    let menu_node = overlay.as_overlay_mut().layout(renderer, VIEWPORT.size());
    let at = Layout::new(&menu_node)
        .children()
        .next()
        .expect("menu")
        .children()
        .nth(row)
        .expect("row")
        .bounds()
        .center();
    let mut messages = Vec::new();
    for event in [
        mouse::Event::CursorMoved { position: at },
        mouse::Event::ButtonPressed(mouse::Button::Left),
        mouse::Event::ButtonReleased(mouse::Button::Left),
    ] {
        let mut shell = Shell::new(&mut messages);
        overlay.as_overlay_mut().update(
            &Event::Mouse(event),
            Layout::new(&menu_node),
            mouse::Cursor::Available(at),
            renderer,
            board,
            &mut shell,
        );
    }
    assert!(
        messages.is_empty(),
        "overlay routes commands through its source"
    );
}
fn redraw() -> Event {
    Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))
}
fn tree<W: Widget<Message, Theme, Renderer>>(widget: &W) -> Tree {
    Tree::new(widget as &dyn Widget<Message, Theme, Renderer>)
}
#[test]
fn overlay_paste_uses_input_on_paste_once_before_child_update() {
    let renderer = renderer();
    let mut board = Board(Some("pasted".into()));
    let target = cosmic::widget::text_input("", "one")
        .on_input(Message::Input)
        .on_paste(Message::Paste);
    let mut menu = text_menu(target, &words(), style()).window_id(window::Id::NONE);
    let mut state = tree(&menu);
    press(&mut menu, &mut state, &renderer, &mut board);
    state.children[0]
        .state
        .downcast_mut::<cosmic::widget::text_input::State>()
        .move_cursor_to_end();
    choose(&mut menu, &mut state, &renderer, &mut board, 2);
    let messages = source_event(
        &mut menu,
        &mut state,
        &renderer,
        &mut board,
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        cosmic::iced::Point::new(500.0, 250.0),
    );
    match &messages[..] {
        [Message::Paste(value)] => assert_eq!(value, "onepasted"),
        [Message::Input(value)] => panic!("on_input fired instead of on_paste: {value}"),
        other => panic!("expected one on_paste message, got {other:?}"),
    }
    assert!(
        source_event(
            &mut menu,
            &mut state,
            &renderer,
            &mut board,
            redraw(),
            POINT
        )
        .is_empty()
    );
}
#[test]
fn overlay_paste_emits_the_editor_action_once() {
    let renderer = renderer();
    let mut board = Board(Some("pasted".into()));
    let content = cosmic::widget::text_editor::Content::<Renderer>::with_text("one");
    let target = cosmic::widget::text_editor::text_editor(&content).on_action(Message::Editor);
    let mut menu = text_menu(target, &words(), style()).window_id(window::Id::NONE);
    let mut state = tree(&menu);
    press(&mut menu, &mut state, &renderer, &mut board);
    choose(&mut menu, &mut state, &renderer, &mut board, 2);
    let messages = source_event(
        &mut menu,
        &mut state,
        &renderer,
        &mut board,
        redraw(),
        POINT,
    );
    assert!(
        matches!(&messages[..], [Message::Editor(cosmic::widget::text_editor::Action::Edit(cosmic::widget::text_editor::Edit::Paste(value)))] if value.as_str() == "pasted")
    );
    assert!(
        source_event(
            &mut menu,
            &mut state,
            &renderer,
            &mut board,
            redraw(),
            POINT
        )
        .is_empty()
    );
}
#[test]
fn custom_rc_choice_is_delivered_only_to_its_source_once() {
    let renderer = renderer();
    let mut board = Board::default();
    let first: Rc<str> = Rc::from("first");
    let second: Rc<str> = Rc::from("second");
    let item = |name: Rc<str>| {
        Menu::new().item(MenuItem {
            label: "Act".into(),
            message: Message::Custom(name),
            icon: None,
            shortcut: None,
            mark: None,
            disabled_reason: None,
            destructive: false,
        })
    };
    let mut a = text_menu(
        cosmic::widget::text_input("", "a").on_input(Message::Input),
        &words(),
        style(),
    )
    .actions(item(first.clone()))
    .window_id(window::Id::NONE);
    let mut b = text_menu(
        cosmic::widget::text_input("", "b").on_input(Message::Input),
        &words(),
        style(),
    )
    .actions(item(second))
    .window_id(window::Id::NONE);
    let mut ta = tree(&a);
    let mut tb = tree(&b);
    press(&mut a, &mut ta, &renderer, &mut board);
    press(&mut b, &mut tb, &renderer, &mut board);
    choose(&mut a, &mut ta, &renderer, &mut board, 0);
    assert!(source_event(&mut b, &mut tb, &renderer, &mut board, redraw(), POINT).is_empty());
    let messages = source_event(&mut a, &mut ta, &renderer, &mut board, redraw(), POINT);
    assert!(matches!(&messages[..], [Message::Custom(value)] if Rc::ptr_eq(value, &first)));
    assert!(source_event(&mut a, &mut ta, &renderer, &mut board, redraw(), POINT).is_empty());
}
#[test]
fn selected_copy_runs_before_the_child_loses_focus() {
    let renderer = renderer();
    let mut board = Board::default();
    let mut menu = text_menu(
        cosmic::widget::text_input("", "selected").on_input(Message::Input),
        &words(),
        style(),
    )
    .window_id(window::Id::NONE);
    let mut state = tree(&menu);
    press(&mut menu, &mut state, &renderer, &mut board);
    choose(&mut menu, &mut state, &renderer, &mut board, 3);
    source_event(
        &mut menu,
        &mut state,
        &renderer,
        &mut board,
        redraw(),
        POINT,
    );
    assert!(
        menu.content
            .text_menu_context(&state.children[0])
            .has_selection,
        "Select All selected the native input"
    );
    press(&mut menu, &mut state, &renderer, &mut board);
    assert!(
        menu.content
            .text_menu_context(&state.children[0])
            .has_selection,
        "right-click preserves the selection"
    );
    choose(&mut menu, &mut state, &renderer, &mut board, 1);
    source_event(
        &mut menu,
        &mut state,
        &renderer,
        &mut board,
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        cosmic::iced::Point::new(500.0, 250.0),
    );
    assert_eq!(board.0.as_deref(), Some("selected"));
}

// Exercise actual menu layout and painting in both themes. The source and
// overlay use the same retained tree as the choice tests above.
fn frame(value: &'static str, selected: bool, reason: bool, theme: &Theme) -> Vec<u8> {
    use cosmic::iced::Color;
    use cosmic::iced::advanced::renderer::{Headless, Style};
    let mut renderer = renderer();
    let mut board = Board(None);
    let mut menu = text_menu(
        cosmic::widget::text_input("", value).on_input(Message::Input),
        &words(),
        style(),
    )
    .window_id(window::Id::NONE);
    let mut state = tree(&menu);
    if selected {
        press(&mut menu, &mut state, &renderer, &mut board);
        choose(&mut menu, &mut state, &renderer, &mut board, 3);
        source_event(
            &mut menu,
            &mut state,
            &renderer,
            &mut board,
            redraw(),
            POINT,
        );
    }
    press(&mut menu, &mut state, &renderer, &mut board);
    let node = menu.layout(
        &mut state,
        &renderer,
        &layout::Limits::new(Size::ZERO, VIEWPORT.size()),
    );
    let colour: Color = theme.cosmic().on_bg_color().into();
    let paint = Style {
        icon_color: colour,
        text_color: colour,
        scale_factor: 1.0,
    };
    menu.draw(
        &state,
        &mut renderer,
        theme,
        &paint,
        Layout::new(&node),
        mouse::Cursor::Available(POINT),
        &VIEWPORT,
    );
    let mut overlay = menu
        .overlay(
            &mut state,
            Layout::new(&node),
            &renderer,
            &VIEWPORT,
            Vector::ZERO,
        )
        .expect("open menu");
    let menu_node = overlay.as_overlay_mut().layout(&renderer, VIEWPORT.size());
    let at = Layout::new(&menu_node)
        .children()
        .next()
        .unwrap()
        .children()
        .nth(2)
        .unwrap()
        .bounds()
        .center();
    let cursor = mouse::Cursor::Available(at);
    if reason {
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        overlay.as_overlay_mut().update(
            &Event::Mouse(mouse::Event::CursorMoved { position: at }),
            Layout::new(&menu_node),
            cursor,
            &renderer,
            &mut board,
            &mut shell,
        );
        overlay.as_overlay_mut().update(
            &redraw(),
            Layout::new(&menu_node),
            cursor,
            &renderer,
            &mut board,
            &mut shell,
        );
    }
    overlay.as_overlay().draw(
        &mut renderer,
        theme,
        &paint,
        Layout::new(&menu_node),
        cursor,
    );
    if reason {
        let mut tip = overlay
            .as_overlay_mut()
            .overlay(Layout::new(&menu_node), &renderer)
            .expect("disabled reason");
        let tip_node = tip.as_overlay_mut().layout(&renderer, VIEWPORT.size());
        tip.as_overlay()
            .draw(&mut renderer, theme, &paint, Layout::new(&tip_node), cursor);
    }
    drop(overlay);
    renderer.screenshot(
        Size::new(VIEWPORT.width as u32, VIEWPORT.height as u32),
        1.0,
        theme.cosmic().bg_color().into(),
    )
}
#[test]
fn selected_and_empty_menus_render_in_light_and_dark_themes() {
    for theme in [Theme::light(), Theme::dark()] {
        let selected = frame("selected", true, false, &theme);
        let empty = frame("", false, false, &theme);
        let reason = frame("", false, true, &theme);
        assert_ne!(
            selected, empty,
            "selected and disabled rows have distinct rendered states"
        );
        assert_ne!(
            reason, empty,
            "the disabled explanation is painted on hover"
        );
        assert!(
            selected
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel.as_slice() != &selected[..4])
        );
        assert!(
            empty
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel.as_slice() != &empty[..4])
        );
    }
}

#[test]
fn newly_restricted_target_rejects_a_previously_enabled_paste() {
    let renderer = renderer();
    let mut board = Board(Some("pasted".into()));
    let target = cosmic::widget::text_input("", "one")
        .on_input(Message::Input)
        .on_paste(Message::Paste);
    let mut menu = text_menu(target, &words(), style()).window_id(window::Id::NONE);
    let mut state = tree(&menu);
    press(&mut menu, &mut state, &renderer, &mut board);
    // A view rebuild can apply the restriction while the old snapshot is open.
    menu.read_only = true;
    choose(&mut menu, &mut state, &renderer, &mut board, 2);
    assert!(
        source_event(
            &mut menu,
            &mut state,
            &renderer,
            &mut board,
            redraw(),
            POINT
        )
        .is_empty()
    );
}
#[test]
fn read_only_restriction_keeps_editor_callbacks_but_disables_mutation() {
    let renderer = renderer();
    let mut board = Board(Some("pasted".into()));
    let content = cosmic::widget::text_editor::Content::<Renderer>::with_text("one");
    let target = cosmic::widget::text_editor::text_editor(&content).on_action(Message::Editor);
    let mut menu = text_menu(target, &words(), style())
        .read_only()
        .window_id(window::Id::NONE);
    let mut state = tree(&menu);
    press(&mut menu, &mut state, &renderer, &mut board);
    choose(&mut menu, &mut state, &renderer, &mut board, 2);
    assert!(
        source_event(
            &mut menu,
            &mut state,
            &renderer,
            &mut board,
            redraw(),
            POINT
        )
        .is_empty()
    );
    // Select all is still offered and goes through the editor's callback.
    press(&mut menu, &mut state, &renderer, &mut board);
    choose(&mut menu, &mut state, &renderer, &mut board, 3);
    let messages = source_event(
        &mut menu,
        &mut state,
        &renderer,
        &mut board,
        redraw(),
        POINT,
    );
    assert!(matches!(
        &messages[..],
        [Message::Editor(
            cosmic::widget::text_editor::Action::SelectAll
        )]
    ));
}

#[test]
fn disabled_paste_exposes_its_reason_as_a_nested_overlay() {
    let renderer = renderer();
    let mut board = Board::default();
    let mut menu = text_menu(
        cosmic::widget::text_input("", "").on_input(Message::Input),
        &words(),
        style(),
    )
    .window_id(window::Id::NONE);
    let mut state = tree(&menu);
    press(&mut menu, &mut state, &renderer, &mut board);
    let node = menu.layout(
        &mut state,
        &renderer,
        &layout::Limits::new(Size::ZERO, VIEWPORT.size()),
    );
    let mut overlay = menu
        .overlay(
            &mut state,
            Layout::new(&node),
            &renderer,
            &VIEWPORT,
            Vector::ZERO,
        )
        .expect("open menu");
    let menu_node = overlay.as_overlay_mut().layout(&renderer, VIEWPORT.size());
    let at = Layout::new(&menu_node)
        .children()
        .next()
        .unwrap()
        .children()
        .nth(2)
        .unwrap()
        .bounds()
        .center();
    let mut messages = Vec::new();
    let mut shell = Shell::new(&mut messages);
    overlay.as_overlay_mut().update(
        &Event::Mouse(mouse::Event::CursorMoved { position: at }),
        Layout::new(&menu_node),
        mouse::Cursor::Available(at),
        &renderer,
        &mut board,
        &mut shell,
    );
    overlay.as_overlay_mut().update(
        &redraw(),
        Layout::new(&menu_node),
        mouse::Cursor::Available(at),
        &renderer,
        &mut board,
        &mut shell,
    );
    assert!(
        overlay
            .as_overlay_mut()
            .overlay(Layout::new(&menu_node), &renderer)
            .is_some(),
        "hovering disabled Paste presents its explanation"
    );
}
