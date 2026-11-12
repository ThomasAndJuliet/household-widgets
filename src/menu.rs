// SPDX-License-Identifier: GPL-3.0-only

//! Resolved household menus (rules 17, 21c, 27–32).
//! Application actions, bindings and icon tables are resolved by the caller.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use cosmic::iced::{Font, Length};
use cosmic::widget::{self, icon, menu::key_bind::KeyBind};
use cosmic::{Element, theme};

const ICON: f32 = 14.0;
const MARK: f32 = 16.0;
const PADDING: f32 = 16.0;
pub const MENU_MIN_WIDTH: u16 = 240;

/// A check box or one choice among radio options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuMark {
    Check(bool),
    Radio(bool),
}

/// An item whose application-specific metadata has already been resolved.
#[derive(Clone)]
pub struct MenuItem<M> {
    pub label: String,
    pub message: M,
    pub icon: Option<icon::Handle>,
    pub shortcut: Option<KeyBind>,
    pub mark: Option<MenuMark>,
    pub disabled_reason: Option<String>,
    pub destructive: bool,
}

#[derive(Clone)]
pub enum Row<M> {
    Item(MenuItem<M>),
    Divider,
    Folder(String, Menu<M>),
}

#[derive(Clone)]
pub struct Menu<M> {
    rows: Vec<Row<M>>,
}

/// The app supplies its bundled marks and live body typography for tooltips.
#[derive(Clone, Copy)]
pub struct MenuStyle<M> {
    pub icon: fn(&'static str) -> icon::Handle,
    pub body: fn(String) -> Element<'static, M>,
}

impl<M> Default for Menu<M> {
    fn default() -> Self {
        Self { rows: Vec::new() }
    }
}

impl<M> Menu<M> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn item(mut self, item: MenuItem<M>) -> Self {
        if let Some(reason) = &item.disabled_reason {
            assert!(!reason.trim().is_empty(), "a disabled menu item says why");
        }
        self.rows.push(Row::Item(item));
        self
    }
    pub fn divider(mut self) -> Self {
        self.rows.push(Row::Divider);
        self
    }
    pub fn folder(mut self, label: impl Into<String>, menu: Self) -> Self {
        self.rows.push(Row::Folder(label.into(), menu));
        self
    }
    pub fn append(mut self, other: Self) -> Self {
        if !other.is_empty() {
            self.rows.push(Row::Divider);
            self.rows.extend(other.rows);
        }
        self
    }
    pub fn is_empty(&self) -> bool {
        !self.rows.iter().any(|r| !matches!(r, Row::Divider))
    }
    pub fn rows(&self) -> &[Row<M>] {
        &self.rows
    }
    pub fn map<N>(self, f: impl Fn(M) -> N) -> Menu<N> {
        fn go<M, N>(menu: Menu<M>, f: &impl Fn(M) -> N) -> Menu<N> {
            Menu {
                rows: menu
                    .rows
                    .into_iter()
                    .map(|row| match row {
                        Row::Item(i) => Row::Item(MenuItem {
                            label: i.label,
                            message: f(i.message),
                            icon: i.icon,
                            shortcut: i.shortcut,
                            mark: i.mark,
                            disabled_reason: i.disabled_reason,
                            destructive: i.destructive,
                        }),
                        Row::Divider => Row::Divider,
                        Row::Folder(label, menu) => Row::Folder(label, go(menu, f)),
                    })
                    .collect(),
            }
        }
        go(self, &f)
    }
    /// The rows in display order, useful for inspecting a resolved menu.
    pub fn outline(&self) -> Vec<String> {
        fn add<M>(rows: &[Row<M>], indent: &str, lines: &mut Vec<String>) {
            let marks = has_marks(rows);
            for at in ordered(rows) {
                match at.map(|i| &rows[i]) {
                    Some(Row::Item(item)) => {
                        let mut line = format!("{indent}{}", item.label);
                        if marks {
                            line = format!(
                                "{indent}[{}] {}",
                                if matches!(
                                    item.mark,
                                    Some(MenuMark::Check(true) | MenuMark::Radio(true))
                                ) {
                                    "x"
                                } else {
                                    " "
                                },
                                item.label
                            );
                        }
                        if let Some(key) = &item.shortcut {
                            line.push_str(&format!("\t{key}"));
                        }
                        if let Some(reason) = &item.disabled_reason {
                            line.push_str(&format!("\tgreyed: {reason}"));
                        }
                        if item.destructive && item.mark.is_none() {
                            line.push_str("\tdestructive");
                        }
                        lines.push(line);
                    }
                    Some(Row::Folder(label, menu)) => {
                        lines.push(format!("{indent}{label} ▸"));
                        add(&menu.rows, &format!("{indent}  "), lines);
                    }
                    Some(Row::Divider) | None => lines.push(format!("{indent}—")),
                }
            }
        }
        let mut lines = Vec::new();
        add(&self.rows, "", &mut lines);
        lines
    }
    /// The common width required by this menu and all its submenus.
    pub fn width(&self) -> u16 {
        width(&self.rows).ceil() as u16
    }
}

pub(crate) fn ordered<M>(rows: &[Row<M>]) -> Vec<Option<usize>> {
    let destructive = |r: &Row<M>| matches!(r, Row::Item(i) if i.destructive && i.mark.is_none());
    let rest = (0..rows.len()).filter(|&i| !destructive(&rows[i]));
    let last: Vec<_> = (0..rows.len()).filter(|&i| destructive(&rows[i])).collect();
    let tail = (!last.is_empty())
        .then_some(None)
        .into_iter()
        .chain(last.into_iter().map(Some));
    let mut result = Vec::new();
    for at in rest.map(Some).chain(tail) {
        if at.is_none_or(|i| matches!(rows[i], Row::Divider)) {
            if matches!(result.last(), Some(Some(_))) {
                result.push(None);
            }
        } else {
            result.push(at);
        }
    }
    if result.last() == Some(&None) {
        result.pop();
    }
    result
}

fn has_marks<M>(rows: &[Row<M>]) -> bool {
    rows.iter()
        .any(|r| matches!(r, Row::Item(i) if i.mark.is_some()))
}

/// Resolved visual data for one household menu row.
#[derive(Clone)]
pub struct MenuRow {
    pub reserve_mark: bool,
    pub mark: Option<icon::Handle>,
    pub icon: Option<icon::Handle>,
    pub label: String,
    pub shortcut: String,
    pub destructive: bool,
    pub selected: bool,
    pub folder: bool,
}

impl MenuRow {
    pub fn label_indent(reserve_mark: bool) -> f32 {
        let space = f32::from(theme::spacing().space_xxs);
        ICON + space + if reserve_mark { MARK + space } else { 0.0 }
    }

    pub fn width(&self) -> f32 {
        let font = if self.selected {
            cosmic::font::semibold()
        } else {
            cosmic::font::default()
        };
        let label = measured_width(&self.label, 14.0, font);
        let trailing = if self.folder {
            MARK
        } else {
            measured_width(&self.shortcut, 14.0, cosmic::font::default())
        };
        2.0 * PADDING
            + Self::label_indent(self.reserve_mark)
            + label
            + if self.folder || !self.shortcut.is_empty() {
                f32::from(theme::spacing().space_l) + trailing
            } else {
                0.0
            }
    }
}

fn width<M>(rows: &[Row<M>]) -> f32 {
    let marks = has_marks(rows);
    rows.iter()
        .map(|r| match r {
            Row::Item(i) => MenuRow {
                reserve_mark: marks,
                mark: None,
                icon: None,
                label: i.label.clone(),
                shortcut: i
                    .shortcut
                    .as_ref()
                    .map_or_else(String::new, ToString::to_string),
                destructive: false,
                selected: matches!(i.mark, Some(MenuMark::Radio(true)))
                    && i.disabled_reason.is_none(),
                folder: false,
            }
            .width(),
            Row::Folder(label, menu) => MenuRow {
                reserve_mark: marks,
                mark: None,
                icon: None,
                label: label.clone(),
                shortcut: String::new(),
                destructive: false,
                selected: false,
                folder: true,
            }
            .width()
            .max(width(&menu.rows)),
            Row::Divider => 0.0,
        })
        .fold(f32::from(MENU_MIN_WIDTH), f32::max)
}

type Widths = HashMap<(u32, Font, String), f32>;
static WIDTHS: LazyLock<Mutex<Widths>> = LazyLock::new(Default::default);
/// Shapes text at the requested size and font, retaining at most 4096 widths.
pub fn measured_width(content: &str, size: f32, font: Font) -> f32 {
    use cosmic::iced::advanced::text::{self, Paragraph as _};
    if content.is_empty() {
        return 0.0;
    }
    let key = (size.to_bits(), font, content.to_owned());
    if let Some(width) = WIDTHS.lock().unwrap().get(&key) {
        return *width;
    }
    let paragraph = <<cosmic::Renderer as text::Renderer>::Paragraph as text::Paragraph>::with_text(
        text::Text {
            content,
            bounds: cosmic::iced::Size::new(f32::INFINITY, f32::INFINITY),
            size: cosmic::iced::Pixels(size),
            line_height: text::LineHeight::default(),
            font,
            align_x: text::Alignment::Default,
            align_y: cosmic::iced::alignment::Vertical::Top,
            shaping: text::Shaping::Advanced,
            wrapping: text::Wrapping::None,
            ellipsize: text::Ellipsize::default(),
        },
    );
    let result = paragraph.min_bounds().width;
    let mut widths = WIDTHS.lock().unwrap();
    if widths.len() >= 4096 {
        widths.clear();
    }
    widths.insert(key, result);
    result
}

impl<M: Clone + 'static> Menu<M> {
    pub fn trees(self, style: MenuStyle<M>) -> Vec<widget::menu::Tree<M>> {
        let marks = has_marks(&self.rows);
        let order = ordered(&self.rows);
        let mut rows: Vec<_> = self.rows.into_iter().map(Some).collect();
        order
            .into_iter()
            .map(|at| match at.and_then(|i| rows[i].take()) {
                Some(Row::Item(item)) => draw(item, marks, &style),
                Some(Row::Folder(label, menu)) => {
                    let children = menu.trees(MenuStyle {
                        icon: style.icon,
                        body: style.body,
                    });
                    let row = MenuRow {
                        reserve_mark: marks,
                        mark: None,
                        icon: None,
                        label,
                        shortcut: String::new(),
                        destructive: false,
                        selected: false,
                        folder: true,
                    }
                    .contents(style.icon);
                    let button = widget::menu::menu_button(row).class(if children.is_empty() {
                        theme::Button::MenuItem
                    } else {
                        theme::Button::MenuFolder
                    });
                    widget::menu::Tree::with_children(
                        widget::RcElementWrapper::new(Element::from(button)),
                        children,
                    )
                }
                Some(Row::Divider) | None => {
                    widget::menu::Tree::from(Element::from(widget::divider::horizontal::light()))
                }
            })
            .collect()
    }
}

fn draw<M: Clone + 'static>(
    item: MenuItem<M>,
    marks: bool,
    style: &MenuStyle<M>,
) -> widget::menu::Tree<M> {
    widget::menu::Tree::from(item_element(item, marks, style))
}

fn item_element<M: Clone + 'static>(
    item: MenuItem<M>,
    marks: bool,
    style: &MenuStyle<M>,
) -> Element<'static, M> {
    let disabled = item.disabled_reason.is_some();
    let chosen = matches!(item.mark, Some(MenuMark::Radio(true))) && !disabled;
    let mark = match item.mark {
        Some(MenuMark::Check(true)) => Some("object-select-symbolic"),
        Some(MenuMark::Radio(true)) => Some("radio-chosen-symbolic"),
        Some(MenuMark::Radio(false)) => Some("radio-empty-symbolic"),
        _ => None,
    };
    let destructive = item.destructive && item.mark.is_none() && !disabled;
    let row = MenuRow {
        reserve_mark: marks,
        mark: mark.map(style.icon),
        icon: item.icon,
        label: item.label,
        shortcut: item
            .shortcut
            .as_ref()
            .map_or_else(String::new, ToString::to_string),
        destructive,
        selected: chosen,
        folder: false,
    }
    .contents(style.icon);
    let mut button = widget::menu::menu_button(row);
    if chosen {
        button = button.selected(true).class(selected_menu_item());
    }
    match item.disabled_reason {
        Some(reason) => crate::tooltip_with(
            button,
            (style.body)(reason),
            widget::tooltip::Position::Bottom,
        ),
        None => Element::from(button.on_press(item.message)),
    }
}

impl MenuRow {
    pub fn contents<M: 'static>(
        self,
        resolve: fn(&'static str) -> icon::Handle,
    ) -> Vec<Element<'static, M>> {
        let Self {
            reserve_mark: marks,
            mark,
            icon,
            label,
            shortcut: key,
            destructive,
            selected: chosen,
            folder,
        } = self;
        let space = theme::spacing().space_xxs;
        let mut row = Vec::with_capacity(7);
        if marks {
            row.push(match mark {
                Some(handle) => widget::icon::icon(handle).size(MARK as u16).into(),
                None => widget::space::horizontal()
                    .width(Length::Fixed(MARK))
                    .into(),
            });
            row.push(widget::space::horizontal().width(space).into());
        }
        row.push(match icon {
            Some(handle) => {
                let mut icon = widget::icon::icon(handle).size(ICON as u16);
                if destructive {
                    icon = icon.class(theme::Svg::Custom(std::rc::Rc::new(|theme| {
                        cosmic::iced::widget::svg::Style {
                            color: Some(theme.cosmic().destructive_text_color().into()),
                        }
                    })));
                }
                icon.into()
            }
            None => widget::space::horizontal()
                .width(Length::Fixed(ICON))
                .into(),
        });
        row.push(widget::space::horizontal().width(space).into());
        let mut text = widget::text(label)
            .shaping(cosmic::iced::advanced::text::Shaping::Advanced)
            .ellipsize(cosmic::iced::advanced::text::Ellipsize::Middle(
                cosmic::iced::advanced::text::EllipsizeHeightLimit::Lines(1),
            ));
        if chosen {
            text = text.font(cosmic::font::semibold());
        }
        if destructive {
            text = text.class(theme::Text::Custom(|theme| {
                cosmic::iced::widget::text::Style {
                    color: Some(theme.cosmic().destructive_text_color().into()),
                    ..Default::default()
                }
            }));
        }
        row.push(text.into());
        row.push(widget::space::horizontal().into());
        if folder {
            row.push(
                widget::icon::icon(resolve("pan-end-symbolic"))
                    .size(16)
                    .into(),
            );
        } else {
            row.push(
                widget::text(key)
                    .shaping(cosmic::iced::advanced::text::Shaping::Advanced)
                    .class(theme::Text::Custom(|theme| {
                        let cosmic = theme.cosmic();
                        let colour = cosmic.on_bg_color();
                        let body = household_tones::text_tone(
                            household_tones::Srgb::new(colour.red, colour.green, colour.blue),
                            cosmic.is_dark,
                        );
                        cosmic::iced::widget::text::Style {
                            color: Some(cosmic::iced::Color {
                                r: body.r,
                                g: body.g,
                                b: body.b,
                                a: 0.68,
                            }),
                            ..Default::default()
                        }
                    }))
                    .into(),
            );
        }
        row
    }
}

fn selected_menu_item() -> theme::Button {
    use cosmic::widget::button::{Catalog, Style};
    fn chosen(mut style: Style, theme: &cosmic::Theme, pressed: bool) -> Style {
        let cosmic = theme.cosmic();
        let component = &cosmic.background(theme.transparent).component;
        let fill = if pressed {
            component.pressed
        } else {
            component.selected_state_color()
        };
        let text = cosmic::iced::Color::from(cosmic.accent_text_color());
        style.background = Some(cosmic::iced::Background::Color(fill.into()));
        style.text_color = Some(text);
        style.icon_color = Some(text);
        style
    }
    theme::Button::Custom {
        active: Box::new(|focused, theme| {
            chosen(
                theme.active(focused, true, &theme::Button::MenuItem),
                theme,
                false,
            )
        }),
        disabled: Box::new(|theme| theme.disabled(&theme::Button::MenuItem)),
        hovered: Box::new(|focused, theme| {
            chosen(
                theme.hovered(focused, true, &theme::Button::MenuItem),
                theme,
                false,
            )
        }),
        pressed: Box::new(|focused, theme| {
            chosen(
                theme.pressed(focused, true, &theme::Button::MenuItem),
                theme,
                true,
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    fn item(label: &str) -> MenuItem<Rc<str>> {
        MenuItem {
            label: label.into(),
            message: Rc::from(label),
            icon: None,
            shortcut: None,
            mark: None,
            disabled_reason: None,
            destructive: false,
        }
    }

    #[test]
    fn measurement_tracks_size_and_weight() {
        let text = "A menu label with a width";
        for font in [cosmic::font::default(), cosmic::font::semibold()] {
            let small = measured_width(text, 14.0, font);
            assert!(small > 0.0);
            assert!(measured_width(text, 20.0, font) > small);
            assert_eq!(measured_width(text, 14.0, font), small);
            assert_eq!(measured_width("", 20.0, font), 0.0);
        }
    }

    #[test]
    fn row_width_fits_the_laid_out_label_and_shortcut() {
        use cosmic::iced::advanced::renderer::Headless;
        use cosmic::iced::advanced::widget::{Id, Operation};
        use cosmic::iced::runtime::user_interface::{Cache, UserInterface};
        use cosmic::iced::{Font, Pixels, Rectangle, Size};
        struct Texts(Vec<(String, Rectangle)>);
        impl Operation for Texts {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                operate(self);
            }
            fn text(&mut self, _: Option<&Id>, bounds: Rectangle, text: &str) {
                self.0.push((text.to_owned(), bounds));
            }
        }
        fn icon(name: &'static str) -> icon::Handle {
            widget::icon::from_name(name).handle()
        }
        for selected in [false, true] {
            let row = MenuRow {
                reserve_mark: true,
                mark: None,
                icon: None,
                label: "A long menu label".into(),
                shortcut: "Ctrl + Shift + C".into(),
                destructive: false,
                selected,
                folder: false,
            };
            let width = row.width().ceil();
            let element: Element<'static, ()> =
                widget::container(widget::menu::menu_button(row.contents(icon)))
                    .width(Length::Fixed(width))
                    .into();
            let mut renderer =
                cosmic::iced::futures::executor::block_on(<cosmic::Renderer as Headless>::new(
                    Font::default(),
                    Pixels(14.0),
                    Some("tiny-skia"),
                ))
                .expect("a headless renderer");
            let mut ui = UserInterface::build(
                element,
                Size::new(width, 60.0),
                Cache::default(),
                &mut renderer,
            );
            let mut texts = Texts(Vec::new());
            ui.operate(&renderer, &mut texts);
            let label = texts
                .0
                .iter()
                .find(|(text, _)| text == "A long menu label")
                .unwrap()
                .1;
            let key = texts
                .0
                .iter()
                .find(|(text, _)| text == "Ctrl + Shift + C")
                .unwrap()
                .1;
            let font = if selected {
                cosmic::font::semibold()
            } else {
                cosmic::font::default()
            };
            assert!(
                label.width + 0.5 >= measured_width("A long menu label", 14.0, font),
                "selected={selected}, row_width={width}, label={label:?}, measured_label={}",
                measured_width("A long menu label", 14.0, font)
            );
            assert!(
                key.width + 0.5
                    >= measured_width("Ctrl + Shift + C", 14.0, cosmic::font::default()),
                "selected={selected}, row_width={width}, shortcut={key:?}, measured_shortcut={}",
                measured_width("Ctrl + Shift + C", 14.0, cosmic::font::default())
            );
            assert!(label.x + label.width <= key.x);
        }
    }

    #[test]
    fn separators_and_destructive_tail_are_normalized_at_each_level() {
        let mut stop = item("Stop");
        stop.destructive = true;
        let sub = Menu::new().item(stop.clone()).item(item("Find"));
        let menu = Menu::new()
            .divider()
            .item(item("Open"))
            .item(stop)
            .divider()
            .divider()
            .folder("More", sub)
            .divider();
        assert_eq!(
            menu.outline(),
            [
                "Open",
                "—",
                "More ▸",
                "  Find",
                "  —",
                "  Stop\tdestructive",
                "—",
                "Stop\tdestructive"
            ]
        );
        assert!(Menu::<Rc<str>>::new().divider().is_empty());
    }

    #[test]
    fn mapping_preserves_marks_disabled_reasons_and_rc_messages() {
        let mut option = item("View");
        option.mark = Some(MenuMark::Radio(true));
        option.disabled_reason = Some("Not now".into());
        let menu = Menu::new()
            .item(option)
            .append(Menu::new().item(item("Copy")));
        let mapped = menu.clone().map(|message| message.len());
        assert_eq!(
            mapped.outline(),
            ["[x] View\tgreyed: Not now", "—", "[ ] Copy"]
        );
        assert!(
            matches!(&mapped.rows()[0], Row::Item(item) if item.message == 4 && item.mark == Some(MenuMark::Radio(true)))
        );
        assert_eq!(menu.rows().len(), 3);
    }

    #[test]
    fn disabled_checked_row_draws_its_mark_and_does_not_activate() {
        use cosmic::iced::advanced::clipboard;
        use cosmic::iced::advanced::renderer::{Headless, Style};
        use cosmic::iced::runtime::user_interface::{Cache, UserInterface};
        use cosmic::iced::{Color, Event, Font, Pixels, Point, Size, mouse};
        fn icon(_: &'static str) -> icon::Handle {
            widget::icon::from_svg_bytes(&b"<svg xmlns='http://www.w3.org/2000/svg' width='16' height='16'><rect x='2' y='2' width='12' height='12' fill='white'/></svg>"[..])
        }
        fn body(text: String) -> Element<'static, Rc<str>> {
            widget::text(text).into()
        }
        fn paint(checked: bool, disabled: bool) -> (Vec<u8>, Vec<Rc<str>>) {
            let mut item = item("Option");
            item.mark = Some(MenuMark::Check(checked));
            item.disabled_reason = disabled.then(|| "Not now".into());
            let element = item_element(item, true, &MenuStyle { icon, body });
            let mut renderer =
                cosmic::iced::futures::executor::block_on(<cosmic::Renderer as Headless>::new(
                    Font::default(),
                    Pixels(14.0),
                    Some("tiny-skia"),
                ))
                .expect("a headless renderer");
            let size = Size::new(240.0, 48.0);
            let mut ui = UserInterface::build(element, size, Cache::default(), &mut renderer);
            let cursor = mouse::Cursor::Available(Point::new(80.0, 16.0));
            let mut messages = Vec::new();
            for event in [
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            ] {
                let _ = ui.update(
                    &[event],
                    cursor,
                    &mut renderer,
                    &mut clipboard::Null,
                    &mut messages,
                );
            }
            ui.draw(
                &mut renderer,
                &cosmic::Theme::default(),
                &Style {
                    text_color: Color::WHITE,
                    ..Style::default()
                },
                mouse::Cursor::Unavailable,
            );
            let pixels = renderer.screenshot(Size::new(240, 48), 1.0, Color::BLACK);
            (pixels, messages)
        }
        let (checked, disabled_messages) = paint(true, true);
        let (unchecked, _) = paint(false, true);
        assert_ne!(
            checked, unchecked,
            "a disabled checked row still draws its mark"
        );
        assert!(
            disabled_messages.is_empty(),
            "a disabled row cannot activate"
        );
        let (_, enabled_messages) = paint(true, false);
        assert_eq!(enabled_messages, [Rc::<str>::from("Option")]);
    }

    #[test]
    fn native_trees_accept_non_send_messages_and_disabled_rows() {
        fn icon(name: &'static str) -> icon::Handle {
            widget::icon::from_name(name).handle()
        }
        fn body(text: String) -> Element<'static, Rc<str>> {
            widget::text(text).into()
        }
        let mut disabled = item("Copy");
        disabled.disabled_reason = Some("Select text".into());
        let trees = Menu::new()
            .item(item("Open"))
            .item(disabled)
            .trees(MenuStyle { icon, body });
        assert_eq!(trees.len(), 2);
    }

    #[test]
    #[should_panic(expected = "says why")]
    fn disabled_items_require_a_reason() {
        let mut disabled = item("Copy");
        disabled.disabled_reason = Some(" ".into());
        let _ = Menu::new().item(disabled);
    }

    #[test]
    fn long_labels_and_submenus_determine_width() {
        let long = "A label long enough to need more than the narrowest menu";
        let menu = Menu::new().folder("More", Menu::new().item(item(long)));
        assert!(menu.width() > MENU_MIN_WIDTH);
        assert_eq!(Menu::new().item(item("Copy")).width(), MENU_MIN_WIDTH);
    }
}
