// SPDX-License-Identifier: GPL-3.0-only
//! Native behavioral checks for logical submenus sharing or separating cards.
use super::*;
use crate::menu::MenuItem;
use cosmic::iced::advanced::{
    mouse,
    renderer::Headless,
    widget::{Operation, operation},
};
use cosmic::iced::runtime::{UserInterface, user_interface};
use cosmic::iced::{Event, Font, Pixels, Point, keyboard};

#[derive(Clone, Debug, PartialEq)]
enum Message {
    Path(Vec<usize>),
    Close(usize),
    Action,
    Dismiss,
    NoFit(usize),
}
fn renderer() -> cosmic::Renderer {
    cosmic::iced::futures::executor::block_on(<cosmic::Renderer as Headless>::new(
        Font::default(),
        Pixels(14.0),
        Some("tiny-skia"),
    ))
    .expect("native renderer")
}
fn item(label: &str) -> MenuItem<Message> {
    MenuItem {
        label: label.into(),
        message: Message::Action,
        icon: None,
        shortcut: None,
        mark: None,
        disabled_reason: None,
        destructive: false,
    }
}
fn icon(name: &'static str) -> widget::icon::Handle {
    widget::icon::from_name(name).handle()
}
fn body(text: String) -> Element<'static, Message> {
    widget::text(text).size(14).into()
}
fn path() -> Vec<usize> {
    vec![row_key("Options", true, 0), row_key("Advanced", true, 0)]
}
fn menu() -> Menu<Message> {
    Menu::new()
        .folder(
            "Options",
            Menu::new().item(item("First choice")).folder(
                "Advanced",
                Menu::new()
                    .item(item("A complete choice label that wraps in a narrow menu"))
                    .item(item("Another choice")),
            ),
        )
        .item(item("Root action"))
}
fn view(
    anchors: &Anchors,
    path: Vec<usize>,
    menu: Menu<Message>,
) -> crate::popup::Host<'static, Message> {
    let trigger = anchors.trigger("adaptive-source", widget::space().width(40).height(20));
    let base = widget::column(vec![widget::space().height(Length::Fill).into(), trigger])
        .height(Length::Fill);
    let panels = MenuSurface::new(
        menu,
        MenuStyle { icon, body },
        anchors.clone(),
        "adaptive".into(),
    )
    .root_candidates(vec![AnchorCandidate::new(
        Side::Above,
        Alignment::StartBounded,
    )])
    .panels(Id::new("adaptive-source"), path, Message::Path, |message| {
        message
    });
    crate::popup::host(base)
        .anchors(anchors.clone())
        .chain(panels)
        .on_close(Message::Close)
        .on_dismiss(Message::Dismiss)
        .on_no_fit(Message::NoFit)
}
#[derive(Default)]
struct Probe {
    focused: Vec<Id>,
    rows: Vec<(Id, Rectangle)>,
    texts: Vec<(String, Rectangle)>,
    scrolls: Vec<(Option<Id>, Rectangle, Rectangle, cosmic::iced::Vector)>,
}
impl Operation for Probe {
    fn text(&mut self, _: Option<&Id>, bounds: Rectangle, text: &str) {
        self.texts.push((text.to_owned(), bounds));
    }
    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: cosmic::iced::Vector,
        _: &mut dyn operation::Scrollable,
    ) {
        self.scrolls
            .push((id.cloned(), bounds, content, translation));
    }
    fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
        visit(self);
    }
    fn focusable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        state: &mut dyn operation::Focusable,
    ) {
        if let Some(id) = id {
            self.rows.push((id.clone(), bounds));
            if state.is_focused() {
                self.focused.push(id.clone());
            }
        }
    }
}
fn inspect(
    anchors: &Anchors,
    path: Vec<usize>,
    menu: Menu<Message>,
    size: Size,
    cache: user_interface::Cache,
    renderer: &mut cosmic::Renderer,
) -> (user_interface::Cache, Probe) {
    let mut ui = UserInterface::build(view(anchors, path, menu), size, cache, renderer);
    let mut probe = Probe::default();
    ui.operate(renderer, &mut probe);
    (ui.into_cache(), probe)
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
#[expect(clippy::too_many_arguments)]
fn event(
    anchors: &Anchors,
    path: Vec<usize>,
    menu: Menu<Message>,
    size: Size,
    cache: user_interface::Cache,
    renderer: &mut cosmic::Renderer,
    event: Event,
    cursor: mouse::Cursor,
) -> (user_interface::Cache, Vec<Message>) {
    let mut ui = UserInterface::build(view(anchors, path, menu), size, cache, renderer);
    let mut messages = Vec::new();
    ui.update(
        &[event],
        cursor,
        renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut messages,
    );
    (ui.into_cache(), messages)
}
fn within(size: Size, surfaces: &[Rectangle]) {
    for surface in surfaces {
        assert!(surface.x >= 0.0 && surface.y >= 0.0);
        assert!(
            surface.x + surface.width <= size.width && surface.y + surface.height <= size.height,
            "complete card including shadow must fit: {surface:?} in {size:?}"
        );
    }
    // Reserved shadow boxes may overlap when actual card edges meet. Assert
    // content surfaces never cover an ancestor, while above still verifies
    // every complete paint box remains inside the window.
    let inset = crate::popup_surface::shadow_padding();
    let visible: Vec<_> = surfaces
        .iter()
        .map(|bounds| Rectangle {
            x: bounds.x + inset.left,
            y: bounds.y + inset.top,
            width: bounds.width - inset.left - inset.right,
            height: bounds.height - inset.top - inset.bottom,
        })
        .collect();
    for (at, surface) in visible.iter().enumerate() {
        for earlier in &visible[..at] {
            assert!(
                surface
                    .intersection(earlier)
                    .is_none_or(|overlap| overlap.width == 0.0 || overlap.height == 0.0),
                "nested cards must not cover ancestors: {surface:?}, {earlier:?}"
            );
        }
    }
}

#[test]
fn measured_chain_uses_beside_then_inline_and_retains_focus_when_resized() {
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let wide = Size::new(1200.0, 500.0);
    // Native measurements fit two cards at 180px; at 170px the complete
    // logical chain fits one 107px-wide, 421.6px-high inline surface.
    let narrow = Size::new(170.0, 500.0);
    let leaf = row_id(
        "adaptive",
        &path(),
        row_key(
            "A complete choice label that wraps in a narrow menu",
            false,
            0,
        ),
    );
    let (cache, probe) = inspect(
        &anchors,
        path(),
        menu(),
        wide,
        user_interface::Cache::new(),
        &mut renderer,
    );
    assert_eq!(
        anchors.surfaces().len(),
        3,
        "all permitted side cards have room"
    );
    within(wide, &anchors.surfaces());
    let insets = crate::popup_surface::shadow_padding();
    for pair in anchors.surfaces().windows(2) {
        let parent_right = pair[0].x + pair[0].width - insets.right;
        let child_left = pair[1].x + insets.left;
        assert_eq!(
            parent_right, child_left,
            "detached visible submenu edges meet without an unexplained gap"
        );
    }
    assert!(
        probe.focused.contains(&leaf),
        "deepest logical level receives initial focus"
    );
    let (cache, probe) = inspect(&anchors, path(), menu(), narrow, cache, &mut renderer);
    assert_eq!(
        anchors.surfaces().len(),
        1,
        "failed side placements expand inside their parent; measured {:?}",
        anchors.surfaces()
    );
    within(narrow, &anchors.surfaces());
    assert!(
        probe.focused.contains(&leaf),
        "native row focus follows the unchanged logical ID"
    );
    let leaf_bounds = anchors
        .visible(&leaf)
        .expect("wrapped leaf remains in the accepted composition");
    assert!(
        leaf_bounds.height > 30.0,
        "full choice text wraps rather than clipping"
    );
    let (cache, messages) = event(
        &anchors,
        path(),
        menu(),
        narrow,
        cache,
        &mut renderer,
        key(keyboard::key::Named::Escape),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(
        messages,
        [Message::Close(2)],
        "Escape closes one logical level within one physical card"
    );
    let (cache, probe) = inspect(
        &anchors,
        path()[..1].to_vec(),
        menu(),
        narrow,
        cache,
        &mut renderer,
    );
    let folder = row_id("adaptive", &path()[..1], path()[1]);
    assert!(
        probe.focused.contains(&folder),
        "closing deepest level restores its owning row focus"
    );
    let (cache, _) = inspect(
        &anchors,
        path()[..1].to_vec(),
        menu(),
        wide,
        cache,
        &mut renderer,
    );
    assert_eq!(anchors.surfaces().len(), 2);
    within(wide, &anchors.surfaces());
    let (_, messages) = event(
        &anchors,
        path()[..1].to_vec(),
        menu(),
        wide,
        cache,
        &mut renderer,
        key(keyboard::key::Named::Enter),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(
        messages,
        [Message::Path(path())],
        "resize does not activate or change the selected value"
    );
}

#[test]
fn open_folder_toggles_closed_after_per_event_rebuilds_in_both_compositions() {
    let mut renderer = renderer();
    for size in [Size::new(233.0, 400.0), Size::new(1000.0, 400.0)] {
        let anchors = Anchors::default();
        let open = path()[..1].to_vec();
        let (mut cache, _) = inspect(
            &anchors,
            open.clone(),
            menu(),
            size,
            user_interface::Cache::new(),
            &mut renderer,
        );
        let folder = row_id("adaptive", &[], open[0]);
        let point = anchors.visible(&folder).expect("owning row").center();
        let mut messages = Vec::new();
        for input in [
            mouse::Event::CursorMoved { position: point },
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            let (next, emitted) = event(
                &anchors,
                open.clone(),
                menu(),
                size,
                cache,
                &mut renderer,
                Event::Mouse(input),
                mouse::Cursor::Available(point),
            );
            cache = next;
            messages.extend(emitted);
        }
        assert_eq!(
            messages,
            [Message::Path(Vec::new())],
            "one click on the owning row closes its branch"
        );
    }
}

#[test]
fn nested_cards_never_reverse_over_an_ancestor_and_tiny_windows_report_no_fit() {
    let mut renderer = renderer();
    for width in [233.0, 320.0, 400.0, 550.0] {
        let anchors = Anchors::default();
        let size = Size::new(width, 400.0);
        let _ = inspect(
            &anchors,
            path(),
            menu(),
            size,
            user_interface::Cache::new(),
            &mut renderer,
        );
        assert!(!anchors.surfaces().is_empty());
        within(size, &anchors.surfaces());
    }
    let anchors = Anchors::default();
    let (_, messages) = event(
        &anchors,
        path(),
        menu(),
        Size::new(40.0, 40.0),
        user_interface::Cache::new(),
        &mut renderer,
        Event::Mouse(mouse::Event::CursorMoved {
            position: Point::ORIGIN,
        }),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(
        messages,
        [Message::NoFit(0)],
        "useful minimum and shadow still constrain the inline form"
    );
}

#[test]
fn a_scrolled_out_owner_is_not_a_stale_path_and_inline_actions_remain_reachable() {
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let size = Size::new(233.0, 220.0);
    let mut many = Menu::new();
    for at in 0..18 {
        many = many.item(item(&format!("Existing action {at}")));
    }
    many = many.folder("Options", Menu::new().item(item("Last choice")));
    let open = path()[..1].to_vec();
    let leaf = row_id("adaptive", &open, row_key("Last choice", false, 0));
    let (mut cache, _) = inspect(
        &anchors,
        open.clone(),
        many.clone(),
        size,
        user_interface::Cache::new(),
        &mut renderer,
    );
    assert_eq!(
        anchors.surfaces().len(),
        1,
        "clipped but existing folder permits inline composition"
    );
    let cursor = mouse::Cursor::Available(anchors.surfaces()[0].center());
    for _ in 0..40 {
        if anchors.visible(&leaf).is_some() {
            break;
        }
        let (next, messages) = event(
            &anchors,
            open.clone(),
            many.clone(),
            size,
            cache,
            &mut renderer,
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -3.0 },
            }),
            cursor,
        );
        assert!(messages.is_empty(), "scrolling never closes a valid branch");
        cache = next;
    }
    let point = anchors
        .visible(&leaf)
        .expect("all inline actions are reachable by explicit scrolling")
        .center();
    let (cache, _) = event(
        &anchors,
        open.clone(),
        many.clone(),
        size,
        cache,
        &mut renderer,
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        mouse::Cursor::Available(point),
    );
    let (_, messages) = event(
        &anchors,
        open,
        many,
        size,
        cache,
        &mut renderer,
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        mouse::Cursor::Available(point),
    );
    assert_eq!(messages, [Message::Action]);
}

#[test]
fn removed_folder_path_is_explicit_no_fit_instead_of_an_embedded_stale_menu() {
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let (_, messages) = event(
        &anchors,
        path(),
        Menu::new().item(item("Root action")),
        Size::new(233.0, 400.0),
        user_interface::Cache::new(),
        &mut renderer,
        key(keyboard::key::Named::ArrowDown),
        mouse::Cursor::Unavailable,
    );
    assert_eq!(messages, [Message::NoFit(1)]);
}

#[test]
fn widening_a_scrolled_inline_menu_keeps_the_valid_offscreen_branch_open() {
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let narrow = Size::new(233.0, 220.0);
    let wide = Size::new(1200.0, 220.0);
    let mut choices = Menu::new();
    for at in 0..20 {
        choices = choices.item(item(&format!("Nested choice {at}")));
    }
    let mut many = Menu::new().folder("Options", choices);
    for at in 0..20 {
        many = many.item(item(&format!("Root action {at}")));
    }
    let open = path()[..1].to_vec();
    let owner = row_id("adaptive", &[], open[0]);
    let leaf = row_id("adaptive", &open, row_key("Nested choice 19", false, 0));
    let (mut cache, _) = inspect(
        &anchors,
        open.clone(),
        many.clone(),
        narrow,
        user_interface::Cache::new(),
        &mut renderer,
    );
    assert_eq!(anchors.surfaces().len(), 1);
    let cursor = mouse::Cursor::Available(anchors.surfaces()[0].center());
    for _ in 0..40 {
        if anchors.visible(&leaf).is_some() {
            break;
        }
        let (next, messages) = event(
            &anchors,
            open.clone(),
            many.clone(),
            narrow,
            cache,
            &mut renderer,
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -2.0 },
            }),
            cursor,
        );
        assert!(messages.is_empty());
        cache = next;
    }
    assert!(
        anchors.visible(&leaf).is_some(),
        "nested choices can be scrolled into view"
    );
    assert!(
        anchors.visible(&owner).is_none(),
        "owning row is outside the native scroll viewport"
    );
    let (cache, before) = inspect(
        &anchors,
        open.clone(),
        many.clone(),
        narrow,
        cache,
        &mut renderer,
    );
    let (cache, after) = inspect(
        &anchors,
        open.clone(),
        many.clone(),
        wide,
        cache,
        &mut renderer,
    );
    assert_eq!(
        anchors.surfaces().len(),
        1,
        "live offscreen source selects the declared inline composition despite a fresh beside trial fitting; before={:?}; after={:?}; source={:?}",
        before.scrolls,
        after.scrolls,
        anchors.visible(&owner)
    );
    let (_, messages) = event(
        &anchors,
        open,
        many,
        wide,
        cache,
        &mut renderer,
        key(keyboard::key::Named::ArrowDown),
        mouse::Cursor::Unavailable,
    );
    assert!(
        messages.is_empty(),
        "widening a valid scrolled branch never reports stale-source NoFit or changes a value"
    );
    within(wide, &anchors.surfaces());
}

#[test]
fn permission_folder_uses_its_measured_preferred_label_width() {
    let mut renderer = renderer();
    let anchors = Anchors::default();
    let menu = Menu::new()
        .folder("Permission mode", Menu::new().item(item("Standard")))
        .item(item("Root action"));
    let panels = MenuSurface::new(
        menu.clone(),
        MenuStyle { icon, body },
        anchors.clone(),
        "preferred-check".into(),
    )
    .panels(Id::new("source"), vec![], Message::Path, |message| message);
    let preferred = panels[0].stages[0]
        .content
        .query(crate::layout::Level::Preferred, &renderer)
        .preferred;
    let (_, probe) = inspect(
        &anchors,
        vec![],
        menu,
        Size::new(600.0, 400.0),
        user_interface::Cache::new(),
        &mut renderer,
    );
    let folder = row_id("adaptive", &[], row_key("Permission mode", true, 0));
    let ordinary = row_id("adaptive", &[], row_key("Root action", false, 0));
    let folder = probe
        .rows
        .iter()
        .find(|(id, _)| id == &folder)
        .expect("permission row")
        .1;
    let ordinary = probe
        .rows
        .iter()
        .find(|(id, _)| id == &ordinary)
        .expect("ordinary row")
        .1;
    assert!(
        (anchors.surfaces()[0].width - preferred.width).abs() < 0.1,
        "the host commits the component's measured preferred width"
    );
    assert!(
        (folder.height - ordinary.height).abs() < 0.1,
        "the full Permission mode label fits on one line beside its arrow at that width; preferred={preferred:?}; folder={folder:?}; ordinary={ordinary:?}; texts={:?}",
        probe.texts
    );
}
