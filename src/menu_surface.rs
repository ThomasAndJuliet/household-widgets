// SPDX-License-Identifier: GPL-3.0-only
//! Adapter from resolved menu metadata to compositional native popup contents.
//! The popup host itself continues to accept arbitrary widget compositions.

use crate::layout::{
    self, Alignment, AnchorCandidate, Attachment, Level, Presentation, PresentationFamily,
    PresentationRequest, Side, Stage, UnsupportedPresentation,
};
use crate::menu::{Menu, MenuMark, MenuStyle, Row};
use crate::menu_parts::{self, RowContour, RowSlots, TooltipChoice};
use crate::popup::{Anchors, FocusTarget, Panel};
use cosmic::iced::advanced::widget::Id;
use cosmic::iced::{Length, Rectangle, Size};
use cosmic::{Element, widget};
use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

const PADDING: f32 = 4.0;

/// A stable key within a menu: inserting a differently named row does not move
/// an open submenu to a different item. Repeated labels use occurrence order.
pub fn row_key(label: &str, folder: bool, occurrence: usize) -> usize {
    let mut hash = DefaultHasher::new();
    (label, folder, occurrence).hash(&mut hash);
    hash.finish() as usize
}

pub fn row_id(namespace: &str, path: &[usize], key: usize) -> Id {
    Id::new(format!("{namespace}:menu:{path:?}:{key}"))
}

fn keys<M>(menu: &Menu<M>) -> Vec<usize> {
    let mut counts = HashMap::new();
    menu.rows()
        .iter()
        .map(|row| {
            let (label, folder) = match row {
                Row::Item(item) => (item.label.as_str(), false),
                Row::Folder(label, _) => (label.as_str(), true),
                Row::Divider => ("", false),
            };
            let occurrence = counts.entry((label, folder)).or_insert(0);
            let key = row_key(label, folder, *occurrence);
            *occurrence += 1;
            key
        })
        .collect()
}

pub struct MenuSurface<M> {
    menu: Menu<M>,
    style: MenuStyle<M>,
    anchors: Anchors,
    namespace: String,
    candidates: Vec<AnchorCandidate>,
    presentation: Presentation,
    choice_body: Option<fn(String) -> Element<'static, M>>,
    emphasized_body: Option<fn(String) -> Element<'static, M>>,
    preferred_width: Option<f32>,
}

impl<M: Clone + 'static> MenuSurface<M> {
    pub fn new(menu: Menu<M>, style: MenuStyle<M>, anchors: Anchors, namespace: String) -> Self {
        Self {
            menu,
            style,
            anchors,
            namespace,
            candidates: Attachment::below(Rectangle::default()).candidates,
            presentation: Presentation::default(),
            choice_body: None,
            emphasized_body: None,
            preferred_width: None,
        }
    }

    /// Menus currently support their ordinary concise presentation only.
    pub fn presentation(
        mut self,
        instance: PresentationRequest,
        immediate_defaults: PresentationRequest,
    ) -> Result<Self, UnsupportedPresentation> {
        self.presentation = menu_family().resolve(instance, immediate_defaults)?;
        Ok(self)
    }

    /// Show exclusive choices through their whole-row appearance instead of
    /// radio glyphs. The callback preserves the application's body typography
    /// and supplies its selected weight. Checkbox marks remain available.
    pub fn choice_rows(mut self, selected_body: fn(String) -> Element<'static, M>) -> Self {
        self.choice_body = Some(selected_body);
        self
    }

    /// Preserve the app's body typography for semibold open disclosure titles.
    /// The choice callback is used when this callback is not supplied.
    pub fn emphasized_body(mut self, body: fn(String) -> Element<'static, M>) -> Self {
        self.emphasized_body = Some(body);
        self
    }

    /// Override this family or instance's preferred card width. Inline
    /// descendants inherit their parent's allocation rather than negotiating
    /// a second width; separate submenu cards retain this policy.
    pub fn preferred_width(mut self, width: f32) -> Self {
        self.preferred_width = Some(width);
        self
    }

    pub fn root_candidates(mut self, candidates: Vec<AnchorCandidate>) -> Self {
        self.candidates = candidates;
        self
    }

    /// `path` contains opaque row keys, not positions in a changing list.
    pub fn panels<'a>(
        self,
        source: Id,
        path: Vec<usize>,
        on_path: impl Fn(Vec<usize>) -> M + 'static,
        on_action: impl Fn(M) -> M + 'static,
    ) -> Vec<Panel<'a, M>> {
        let mut content = Content {
            menu: self.menu,
            style: self.style,
            anchors: self.anchors,
            namespace: self.namespace,
            path: Vec::new(),
            open: path.clone(),
            on_path: Rc::new(on_path),
            on_action: Rc::new(on_action),
            presentation: self.presentation,
            choice_body: self.choice_body,
            emphasized_body: self.emphasized_body,
            preferred_width: self.preferred_width,
        };
        let mut definitions = vec![(content.clone(), source, self.candidates)];
        let mut stale = None;
        for key in path {
            let submenu = content
                .menu
                .rows()
                .iter()
                .zip(keys(&content.menu))
                .find_map(|(row, candidate)| {
                    if key != candidate {
                        return None;
                    }
                    if let Row::Folder(_, submenu) = row {
                        Some(submenu.clone())
                    } else {
                        None
                    }
                });
            let source = row_id(&content.namespace, &content.path, key);
            content.path.push(key);
            let Some(submenu) = submenu else {
                // Preserve a missing source in the requested chain so the host
                // reports NoFit and the application closes the stale session.
                stale = Some(
                    Panel::new(Attachment::beside(Rectangle::default()), Vec::new()).source(source),
                );
                break;
            };
            content.menu = submenu;
            let candidates = [Side::Right, Side::Left]
                .into_iter()
                .map(|side| AnchorCandidate::new(side, Alignment::StartBounded))
                .collect();
            definitions.push((content.clone(), source, candidates));
        }
        let count = definitions.len();
        let mut panels: Vec<_> = definitions
            .into_iter()
            .enumerate()
            .map(|(index, (content, source, candidates))| {
                let mut panel = content.panel(source, candidates).embedded_compositions(
                    (1..count - index)
                        .map(|embedded| content.stages(embedded))
                        .collect(),
                );
                if index > 0 {
                    panel = panel.beside_previous_surface();
                }
                panel
            })
            .collect();
        if let Some(stale) = stale {
            panels.push(stale);
        }
        panels
    }
}

#[derive(Clone)]
struct Content<M> {
    menu: Menu<M>,
    style: MenuStyle<M>,
    anchors: Anchors,
    namespace: String,
    path: Vec<usize>,
    open: Vec<usize>,
    on_path: Rc<dyn Fn(Vec<usize>) -> M>,
    on_action: Rc<dyn Fn(M) -> M>,
    presentation: Presentation,
    choice_body: Option<fn(String) -> Element<'static, M>>,
    emphasized_body: Option<fn(String) -> Element<'static, M>>,
    preferred_width: Option<f32>,
}

impl<M: Clone + 'static> Content<M> {
    fn panel<'a>(&self, source: Id, candidates: Vec<AnchorCandidate>) -> Panel<'a, M> {
        let keys = keys(&self.menu);
        let targets = crate::menu::ordered(self.menu.rows())
            .into_iter()
            .flatten()
            .filter_map(|at| {
                let id = row_id(&self.namespace, &self.path, keys[at]);
                match &self.menu.rows()[at] {
                    Row::Item(item) => {
                        let target = FocusTarget::new(id);
                        Some(if item.disabled_reason.is_none() {
                            target.activate((self.on_action)(item.message.clone()))
                        } else {
                            target
                        })
                    }
                    Row::Folder(_, _) => {
                        let mut path = self.path.clone();
                        path.push(keys[at]);
                        let open = (self.on_path)(path.clone());
                        let toggle = if self.open.starts_with(&path) {
                            (self.on_path)(self.path.clone())
                        } else {
                            open.clone()
                        };
                        Some(FocusTarget::new(id).activate(toggle).open(open))
                    }
                    _ => None,
                }
            })
            .collect();
        let mut hash = DefaultHasher::new();
        (&self.namespace, &self.path).hash(&mut hash);
        Panel::new(
            Attachment::new(Rectangle::default(), candidates),
            self.stages(0),
        )
        .source(source)
        .surface_insets(crate::popup_surface::shadow_padding())
        .key(hash.finish())
        .focus_targets(targets)
        .focus_first()
    }

    fn stages<'a>(&self, embedded: usize) -> Vec<Stage<'a, M>> {
        let normal = self.clone();
        let scrolling = self.clone();
        let minimum = self.clone();
        let preferred = self.clone();
        let scroll_minimum = self.clone();
        let scroll_preferred = self.clone();
        let scroll_minimum_for_width = self.clone();
        let preferred_width = self.preferred_width;
        vec![
            Stage::new(
                Level::Preferred,
                menu_family()
                    .prepare(
                        self.presentation.into(),
                        PresentationRequest::default(),
                        move |_| {
                            let child = layout::child(move |_| normal.view(false, embedded))
                                .minimum_width_with(move |level, renderer, _| {
                                    preferred
                                        .probe(embedded, false)
                                        .query(level, renderer)
                                        .preferred
                                        .width
                                })
                                .minimum_width_with(move |level, renderer, _| {
                                    minimum
                                        .probe(embedded, true)
                                        .query(level, renderer)
                                        .preferred
                                        .width
                                })
                                // Repeated native chrome subtraction must
                                // not offer a fraction below a text run.
                                .round_width_up();
                            let child = if let Some(width) = preferred_width {
                                child.preferred_width(width)
                            } else {
                                child
                            };
                            crate::popup_surface::reserve_shadow(child)
                        },
                    )
                    .expect("validated menu presentation"),
            ),
            Stage::new(
                Level::Mild,
                menu_family()
                    .prepare(
                        self.presentation.into(),
                        PresentationRequest::default(),
                        move |_| {
                            let child = layout::scroll_child_with_minimum(
                                move |_| scrolling.view(true, embedded),
                                move |level, renderer| {
                                    Size::new(
                                        scroll_minimum
                                            .probe(embedded, true)
                                            .query(level, renderer)
                                            .preferred
                                            .width,
                                        72.0,
                                    )
                                },
                            )
                            .minimum_width_with(move |level, renderer, _| {
                                scroll_preferred
                                    .probe(embedded, false)
                                    .query(level, renderer)
                                    .preferred
                                    .width
                            })
                            .minimum_width_with(move |level, renderer, _| {
                                scroll_minimum_for_width
                                    .probe(embedded, true)
                                    .query(level, renderer)
                                    .preferred
                                    .width
                            })
                            .round_width_up();
                            let child = if let Some(width) = preferred_width {
                                child.preferred_width(width)
                            } else {
                                child
                            };
                            crate::popup_surface::reserve_shadow(child).grow_height(1.0)
                        },
                    )
                    .expect("validated menu presentation"),
            ),
        ]
    }

    /// A readable label run (first word, up to eight characters), measured
    /// through the real body typography with every actual gutter and trailing
    /// control, defines this family's useful width. Full labels are never
    /// shortened in the selected composition; native text wraps at that width.
    fn probe(&self, embedded: usize, minimum: bool) -> layout::Child<'static, M> {
        let content = self.clone();
        layout::child(move |_| {
            widget::container(content.rows_view(embedded, Some(minimum), false))
                .padding([PADDING, 0.0])
                .into()
        })
    }

    fn view(&self, scrolling: bool, embedded: usize) -> Element<'static, M> {
        let body = self.rows_view(embedded, None, false);
        let inner: Element<'static, M> = if scrolling {
            let scroll: Element<'static, M> = widget::scrollable(body)
                .id(Id::new(format!(
                    "{}:menu-scroll:{:?}",
                    self.namespace, self.path
                )))
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
            crate::popup::state_scope(
                Id::new(format!(
                    "{}:menu-scroll-state:{:?}",
                    self.namespace, self.path
                )),
                scroll,
            )
        } else {
            body
        };
        let card = widget::container(inner)
            .padding([PADDING, 0.0])
            .width(Length::Fill)
            .max_width(if self.preferred_width.is_some() {
                f32::INFINITY
            } else {
                f32::from(self.menu.width())
            })
            .class(crate::popup_surface::style());
        if scrolling {
            card.height(Length::Fill).into()
        } else {
            card.into()
        }
    }

    fn rows_view(&self, embedded: usize, probe: Option<bool>, framed: bool) -> Element<'static, M> {
        let rows = self.menu.rows();
        let keys = keys(&self.menu);
        let frame_inset = crate::look::Look::normal()
            .border(&cosmic::theme::active(), crate::look::State::Focused)
            .width;
        let slots = RowSlots {
            mark_width: rows
                .iter()
                .any(|row| matches!(row, Row::Item(item) if item.mark.is_some() && !(self.choice_body.is_some() && matches!(item.mark, Some(MenuMark::Radio(_))))))
                .then_some(16.0),
            icon_width: rows
                .iter()
                .any(|row| matches!(row, Row::Item(item) if item.icon.is_some()))
                .then_some(14.0),
            gap: 8.0,
        };
        let mut groups = vec![Vec::new()];
        let ordered = crate::menu::ordered(rows);
        let last = ordered.iter().rev().flatten().next().copied();
        for at in ordered {
            let Some(at) = at else {
                groups.push(Vec::new());
                continue;
            };
            let id = row_id(&self.namespace, &self.path, keys[at]);
            let branch_open = self.open.get(self.path.len()) == Some(&keys[at])
                && matches!(rows[at], Row::Folder(_, _));
            let inline = embedded > 0 && branch_open;
            let contour = if inline {
                RowContour::OpenTitle
            } else if framed && last == Some(at) {
                RowContour::FrameBottom { inset: frame_inset }
            } else {
                RowContour::Rounded
            };
            let selected = matches!(&rows[at], Row::Item(item) if matches!(item.mark, Some(MenuMark::Radio(true))));
            let (mark, icon, label, trailing, message, reason, folder, destructive) = match &rows
                [at]
            {
                Row::Divider => {
                    groups.push(Vec::new());
                    continue;
                }
                Row::Item(item) => {
                    let mark = match item.mark {
                        Some(MenuMark::Check(true)) => Some("object-select-symbolic"),
                        Some(MenuMark::Radio(true)) if self.choice_body.is_none() => {
                            Some("radio-chosen-symbolic")
                        }
                        Some(MenuMark::Radio(false)) if self.choice_body.is_none() => {
                            Some("radio-empty-symbolic")
                        }
                        _ => None,
                    }
                    .map(|name| widget::icon::icon((self.style.icon)(name)).size(16).into());
                    let icon = item
                        .icon
                        .clone()
                        .map(|icon| widget::icon::icon(icon).size(14).into());
                    let trailing = item
                        .shortcut
                        .as_ref()
                        .map(|key| (self.style.body)(key.to_string()));
                    (
                        mark,
                        icon,
                        item.label.clone(),
                        trailing,
                        item.disabled_reason
                            .is_none()
                            .then(|| (self.on_action)(item.message.clone())),
                        item.disabled_reason.clone(),
                        false,
                        item.destructive,
                    )
                }
                Row::Folder(label, _) => {
                    let mut path = self.path.clone();
                    path.push(keys[at]);
                    let expanded = self.open.starts_with(&path);
                    let toggle = if expanded { self.path.clone() } else { path };
                    (
                        None,
                        None,
                        label.clone(),
                        Some(
                            widget::icon::icon((self.style.icon)(if expanded && embedded > 0 {
                                "pan-down-symbolic"
                            } else {
                                "pan-end-symbolic"
                            }))
                            .size(14)
                            .into(),
                        ),
                        Some((self.on_path)(toggle)),
                        None,
                        true,
                        false,
                    )
                }
            };
            let label = if probe == Some(true) {
                label
                    .split_whitespace()
                    .next()
                    .unwrap_or(&label)
                    .chars()
                    .take(8)
                    .collect()
            } else {
                label
            };
            let label = if branch_open {
                if let Some(body) = self.emphasized_body.or(self.choice_body) {
                    body(label)
                } else {
                    widget::text(label).font(cosmic::font::semibold()).into()
                }
            } else if selected {
                if let Some(body) = self.choice_body.or(self.emphasized_body) {
                    body(label)
                } else {
                    widget::text(label).font(cosmic::font::semibold()).into()
                }
            } else if destructive {
                widget::text(label)
                    .size(14)
                    .class(cosmic::theme::Text::Color(
                        cosmic::theme::active().cosmic().destructive_color().into(),
                    ))
                    .into()
            } else {
                (self.style.body)(label)
            };
            if probe.is_some() {
                let mut pieces: Vec<Element<'static, M>> = Vec::new();
                for (width, content) in [(slots.mark_width, mark), (slots.icon_width, icon)] {
                    if let Some(width) = width {
                        pieces.push(
                            widget::container(content.unwrap_or_else(|| widget::space().into()))
                                .width(width)
                                .into(),
                        );
                    }
                }
                let mut cell = vec![label];
                if let Some(trailing) = trailing {
                    cell.push(trailing);
                }
                pieces.push(widget::row(cell).spacing(slots.gap).into());
                let row = widget::container(widget::row(pieces).spacing(slots.gap))
                    .padding([6.0, 8.0 + if inline { frame_inset } else { 0.0 }])
                    .into();
                if inline && let Row::Folder(_, submenu) = &rows[at] {
                    let mut child = self.clone();
                    child.menu = submenu.clone();
                    child.path.push(keys[at]);
                    let group =
                        menu_parts::inline_group(row, child.rows_view(embedded - 1, probe, true));
                    groups
                        .last_mut()
                        .unwrap()
                        .push(inset_group(group, framed, last == Some(at)));
                } else {
                    groups.last_mut().unwrap().push(row);
                }
                continue;
            }
            let row = menu_parts::row_content(slots, mark, icon, label, trailing);
            let activation = message.clone();
            let selected_style = selected;
            let row = menu_parts::action_row_with(
                row,
                TooltipChoice::NoTooltip,
                message,
                move |button| {
                    button
                        .padding([6.0, 8.0 + if inline { frame_inset } else { 0.0 }])
                        .class(menu_parts::row_style(selected_style, branch_open, contour))
                },
            );
            let target = menu_parts::row_target(id.clone(), row, contour);
            let row = match activation {
                Some(activation) => target.on_activate(activation).into(),
                None => target.into(),
            };
            let row = if let Some(reason) = reason {
                let body = self.style.body;
                let text = crate::hover_card::normal_text_box_with(
                    reason,
                    PresentationRequest::default(),
                    PresentationRequest::default(),
                    body,
                )
                .expect("default text presentation");
                crate::hover_card::hover_box(
                    self.anchors.clone(),
                    id.clone(),
                    row,
                    text,
                    vec![Side::Right, Side::Left, Side::Above, Side::Below]
                        .into_iter()
                        .map(|side| AnchorCandidate::new(side, Alignment::StartBounded).gap(4.0))
                        .collect(),
                )
            } else {
                row
            };
            let row = if folder {
                self.anchors.trigger(id, row)
            } else {
                self.anchors.anchor(id, row)
            };
            if inline && let Row::Folder(_, submenu) = &rows[at] {
                let mut child = self.clone();
                child.menu = submenu.clone();
                child.path.push(keys[at]);
                let group =
                    menu_parts::inline_group(row, child.rows_view(embedded - 1, probe, true));
                groups
                    .last_mut()
                    .unwrap()
                    .push(inset_group(group, framed, last == Some(at)));
            } else {
                groups.last_mut().unwrap().push(row);
            }
        }
        if probe.is_some() {
            widget::column(groups.into_iter().flatten()).into()
        } else {
            menu_parts::groups(groups, || widget::divider::horizontal::light().into())
        }
    }
}

fn inset_group<M: 'static>(
    group: Element<'static, M>,
    nested: bool,
    last: bool,
) -> Element<'static, M> {
    if !nested {
        return group;
    }
    let inset = f32::from(cosmic::theme::active().cosmic().spacing.space_xxs);
    widget::container(group)
        .width(Length::Fill)
        .padding(cosmic::iced::Padding {
            top: 0.0,
            left: inset,
            right: inset,
            bottom: if last { inset } else { 0.0 },
        })
        .into()
}

/// The supported presentation catalogue for the menu component.
pub fn menu_family() -> PresentationFamily {
    let normal = Presentation::default();
    PresentationFamily::new(normal, vec![normal])
}

#[cfg(test)]
mod presentation_tests {
    use super::*;
    use crate::layout::{Detail, Emphasis};

    #[test]
    fn menu_rejects_unsupported_defaults_and_honours_instance_override() {
        let family = menu_family();
        let emphasis = PresentationRequest {
            emphasis: Some(Emphasis::Emphasised),
            detail: None,
        };
        assert!(
            family
                .resolve(PresentationRequest::default(), emphasis)
                .is_err()
        );
        let normal = PresentationRequest {
            emphasis: Some(Emphasis::Normal),
            detail: None,
        };
        assert_eq!(
            family.resolve(normal, emphasis).unwrap(),
            Presentation::default()
        );
        assert!(
            family
                .resolve(
                    PresentationRequest {
                        emphasis: None,
                        detail: Some(Detail::Verbose)
                    },
                    PresentationRequest::default()
                )
                .is_err()
        );
    }
    #[test]
    fn family_preference_is_overrideable_without_changing_the_useful_floor() {
        use cosmic::iced::advanced::renderer::Headless;
        use cosmic::iced::{Font, Pixels};
        let renderer = cosmic::iced::futures::executor::block_on(
            <cosmic::Renderer as Headless>::new(Font::default(), Pixels(14.0), Some("tiny-skia")),
        )
        .expect("native renderer");
        fn icon(name: &'static str) -> widget::icon::Handle {
            widget::icon::from_name(name).handle()
        }
        fn body(text: String) -> Element<'static, ()> {
            widget::text(text).into()
        }
        let measure = |width: Option<f32>| {
            let menu = Menu::new().item(crate::menu::MenuItem {
                label: "Short".into(),
                message: (),
                icon: None,
                shortcut: None,
                mark: None,
                disabled_reason: None,
                destructive: false,
            });
            let surface = MenuSurface::new(
                menu,
                MenuStyle { icon, body },
                Anchors::default(),
                "width-policy".into(),
            );
            let surface = if let Some(width) = width {
                surface.preferred_width(width)
            } else {
                surface
            };
            surface
                .panels(Id::new("source"), vec![], |_| (), |_| ())
                .remove(0)
                .stages
                .remove(0)
                .content
                .query(Level::Preferred, &renderer)
        };
        let natural = measure(None);
        let family = measure(Some(240.0));
        let instance = measure(Some(180.0));
        let shadow = crate::popup_surface::shadow_padding();
        let chrome = shadow.left + shadow.right;
        assert!(natural.preferred.width < instance.preferred.width);
        assert_eq!(family.preferred.width, 240.0 + chrome);
        assert_eq!(instance.preferred.width, 180.0 + chrome);
        assert_eq!(family.minimum.width, instance.minimum.width);
        assert_eq!(family.minimum.width, natural.minimum.width);
    }

    #[test]
    fn choice_rows_drop_radio_gutters_but_preserve_checkbox_alignment() {
        use cosmic::iced::advanced::{
            Layout, Widget,
            renderer::Headless,
            widget::{Operation, Tree},
        };
        use cosmic::iced::{Font, Pixels};
        let renderer = cosmic::iced::futures::executor::block_on(
            <cosmic::Renderer as Headless>::new(Font::default(), Pixels(14.0), Some("tiny-skia")),
        )
        .expect("native renderer");
        fn icon(name: &'static str) -> widget::icon::Handle {
            widget::icon::from_name(name).handle()
        }
        fn body(text: String) -> Element<'static, ()> {
            widget::text(text).into()
        }
        fn chosen(text: String) -> Element<'static, ()> {
            widget::text(text).font(cosmic::font::semibold()).into()
        }
        #[derive(Default)]
        struct Labels(Vec<Rectangle>);
        impl Operation for Labels {
            fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
                visit(self);
            }
            fn text(&mut self, _: Option<&Id>, bounds: Rectangle, text: &str) {
                if text == "First" || text == "Second" {
                    self.0.push(bounds);
                }
            }
        }
        let measure = |choice_rows: bool, second_mark: MenuMark| {
            let item = |label: &str, mark| crate::menu::MenuItem {
                label: label.into(),
                message: (),
                icon: None,
                shortcut: None,
                mark: Some(mark),
                disabled_reason: None,
                destructive: false,
            };
            let menu = Menu::new()
                .item(item("First", MenuMark::Radio(true)))
                .item(item("Second", second_mark));
            let mut surface = MenuSurface::new(
                menu,
                MenuStyle { icon, body },
                Anchors::default(),
                "choice-test".into(),
            );
            if choice_rows {
                surface = surface.choice_rows(chosen);
            }
            let panel = surface
                .panels(Id::new("choice-source"), vec![], |_| (), |_| ())
                .remove(0);
            let mut root = layout::stages(panel.stages);
            let mut tree = Tree::empty();
            let node = root.layout(
                &mut tree,
                &renderer,
                &cosmic::iced::advanced::layout::Limits::new(Size::ZERO, Size::new(320.0, 300.0)),
            );
            assert!(root.selection().unwrap().is_ok());
            let mut labels = Labels::default();
            root.operate(&mut tree, Layout::new(&node), &renderer, &mut labels);
            assert_eq!(labels.0.len(), 2);
            assert_eq!(
                labels.0[0].x, labels.0[1].x,
                "every row shares its label origin"
            );
            labels.0[0].x
        };
        let marks = measure(false, MenuMark::Radio(false));
        let choices = measure(true, MenuMark::Radio(false));
        assert_eq!(
            marks - choices,
            24.0,
            "the optional 16px mark slot and 8px gap are absent"
        );
        assert_eq!(
            measure(true, MenuMark::Check(false)),
            marks,
            "a mixed menu still reserves its checkbox slot"
        );
    }

    #[test]
    fn selected_radio_has_the_same_semibold_label_and_wash_with_or_without_its_mark_column() {
        use cosmic::iced::advanced::{
            Layout, Shell, Widget, mouse,
            renderer::Headless,
            widget::{Operation, Tree},
        };
        use cosmic::iced::{Color, Event, Font, Pixels};
        fn icon(name: &'static str) -> widget::icon::Handle {
            widget::icon::from_name(name).handle()
        }
        fn body(text: String) -> Element<'static, ()> {
            widget::text(text).size(14).into()
        }
        fn semibold(text: String) -> Element<'static, ()> {
            widget::text(text)
                .size(14)
                .font(cosmic::font::semibold())
                .into()
        }
        #[derive(Default)]
        struct Label(Option<Rectangle>);
        impl Operation for Label {
            fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
                visit(self);
            }
            fn text(&mut self, _: Option<&Id>, bounds: Rectangle, text: &str) {
                if text == "A selected menu choice" {
                    self.0 = Some(bounds);
                }
            }
        }
        let render = |choice_rows: bool| {
            let mut renderer =
                cosmic::iced::futures::executor::block_on(<cosmic::Renderer as Headless>::new(
                    Font::default(),
                    Pixels(14.0),
                    Some("tiny-skia"),
                ))
                .expect("native renderer");
            let item = crate::menu::MenuItem {
                label: "A selected menu choice".into(),
                message: (),
                icon: None,
                shortcut: None,
                mark: Some(MenuMark::Radio(true)),
                disabled_reason: None,
                destructive: false,
            };
            let mut surface = MenuSurface::new(
                Menu::new().item(item),
                MenuStyle { icon, body },
                Anchors::default(),
                "selection".into(),
            );
            if choice_rows {
                surface = surface.choice_rows(semibold);
            }
            let panel = surface
                .panels(Id::new("source"), vec![], |_| (), |_| ())
                .remove(0);
            let mut view = layout::stages(panel.stages);
            let mut tree = Tree::empty();
            let size = Size::new(320.0, 100.0);
            let node = view.layout(
                &mut tree,
                &renderer,
                &cosmic::iced::advanced::layout::Limits::new(Size::ZERO, size),
            );
            assert!(view.selection().unwrap().is_ok());
            let mut label = Label::default();
            view.operate(&mut tree, Layout::new(&node), &renderer, &mut label);
            let bounds = label.0.expect("selected label in native tree");
            view.update(
                &mut tree,
                &Event::Window(cosmic::iced::window::Event::RedrawRequested(
                    std::time::Instant::now(),
                )),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut cosmic::iced::advanced::clipboard::Null,
                &mut Shell::new(&mut Vec::new()),
                &Rectangle::with_size(size),
            );
            view.draw(
                &tree,
                &mut renderer,
                &cosmic::Theme::light(),
                &cosmic::iced::advanced::renderer::Style::default(),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &Rectangle::with_size(size),
            );
            let pixels = renderer.screenshot(Size::new(320, 100), 1.0, Color::WHITE);
            let mut label_pixels = Vec::new();
            for y in bounds.y as usize..bounds.y as usize + bounds.height as usize {
                for x in bounds.x as usize..bounds.x as usize + 100 {
                    label_pixels.extend_from_slice(&pixels[(y * 320 + x) * 4..][..4]);
                }
            }
            (bounds, label_pixels)
        };
        let (marked, marked_pixels) = render(false);
        let (unmarked, unmarked_pixels) = render(true);
        assert_eq!(
            marked.x - unmarked.x,
            24.0,
            "choice_rows removes only the mark gutter"
        );
        assert_eq!(
            marked_pixels, unmarked_pixels,
            "the default radio selection matches the explicitly semibold no-mark selection, including its grey wash and accent text"
        );
    }

    #[test]
    fn side_open_title_is_semibold_and_muted_while_its_selected_choice_keeps_the_grey_wash() {
        use cosmic::iced::advanced::{
            Layout, Shell, mouse,
            renderer::Headless,
            widget::{Operation, Tree},
        };
        use cosmic::iced::{Color, Event, Font, Pixels};
        fn icon(name: &'static str) -> widget::icon::Handle {
            widget::icon::from_name(name).handle()
        }
        fn body(text: String) -> Element<'static, ()> {
            widget::text(text).size(14).into()
        }
        fn semibold(text: String) -> Element<'static, ()> {
            widget::text(text)
                .size(14)
                .font(cosmic::font::semibold())
                .into()
        }
        #[derive(Default)]
        struct Labels(HashMap<String, Rectangle>);
        impl Operation for Labels {
            fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
                visit(self);
            }
            fn text(&mut self, _: Option<&Id>, bounds: Rectangle, text: &str) {
                self.0.insert(text.into(), bounds);
            }
        }
        let paint = |mut element: Element<'static, ()>| {
            let mut renderer =
                cosmic::iced::futures::executor::block_on(<cosmic::Renderer as Headless>::new(
                    Font::default(),
                    Pixels(14.0),
                    Some("tiny-skia"),
                ))
                .expect("native renderer");
            let mut tree = Tree::new(element.as_widget());
            let size = Size::new(200.0, 120.0);
            let node = element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &cosmic::iced::advanced::layout::Limits::new(Size::ZERO, size),
            );
            let mut labels = Labels::default();
            element
                .as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut labels);
            element.as_widget_mut().update(
                &mut tree,
                &Event::Window(cosmic::iced::window::Event::RedrawRequested(
                    std::time::Instant::now(),
                )),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut cosmic::iced::advanced::clipboard::Null,
                &mut Shell::new(&mut Vec::new()),
                &Rectangle::with_size(size),
            );
            element.as_widget().draw(
                &tree,
                &mut renderer,
                &cosmic::Theme::light(),
                &cosmic::iced::advanced::renderer::Style::default(),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &Rectangle::with_size(size),
            );
            (
                renderer.screenshot(Size::new(200, 120), 1.0, Color::WHITE),
                labels.0,
            )
        };
        let item = crate::menu::MenuItem {
            label: "High".into(),
            message: (),
            icon: None,
            shortcut: None,
            mark: Some(MenuMark::Radio(true)),
            disabled_reason: None,
            destructive: false,
        };
        let content = Content {
            menu: Menu::new().folder("Effort", Menu::new().item(item)),
            style: MenuStyle { icon, body },
            anchors: Anchors::default(),
            namespace: "side-open".into(),
            path: vec![],
            open: vec![row_key("Effort", true, 0)],
            on_path: Rc::new(|_| ()),
            on_action: Rc::new(|_| ()),
            presentation: Presentation::default(),
            choice_body: Some(semibold),
            emphasized_body: Some(semibold),
            preferred_width: None,
        };
        let (side, side_labels) = paint(content.rows_view(0, None, false));
        let reference = menu_parts::action_row_with(
            menu_parts::row_content(
                RowSlots::default(),
                None,
                None,
                semibold("Effort".into()),
                None,
            ),
            TooltipChoice::NoTooltip,
            Some(()),
            |button| {
                button.padding([6.0, 8.0]).class(menu_parts::row_style(
                    false,
                    true,
                    RowContour::Rounded,
                ))
            },
        );
        let (reference, reference_labels) = paint(reference);
        let crop = |pixels: &[u8], bounds: Rectangle| {
            let mut result = Vec::new();
            for y in bounds.y as usize..bounds.y as usize + bounds.height as usize {
                for x in bounds.x as usize..bounds.x as usize + 100 {
                    result.extend_from_slice(&pixels[(y * 200 + x) * 4..][..4]);
                }
            }
            result
        };
        assert_eq!(
            crop(&side, side_labels["Effort"]),
            crop(&reference, reference_labels["Effort"]),
            "a logical side-open title has the explicit semibold label and muted-fill appearance"
        );
        let (inline, labels) = paint(content.rows_view(1, None, false));
        let pixel = |x: usize, y: usize| &inline[(y * 200 + x) * 4..][..4];
        let selected = pixel(120, labels["High"].center_y() as usize);
        let title = pixel(120, labels["Effort"].center_y() as usize);
        assert_eq!(selected[0], selected[1]);
        assert_eq!(
            selected[1], selected[2],
            "the selected option retains its grey wash"
        );
        assert_ne!(
            selected, title,
            "selection and the open title remain separate states"
        );
    }

    #[test]
    fn real_menu_box_queries_and_commits_native_geometry() {
        use cosmic::iced::advanced::{
            Widget, layout as native_layout, renderer::Headless, widget::Tree,
        };
        use cosmic::iced::{Font, Pixels, Size};
        let mut renderer = cosmic::iced::futures::executor::block_on(
            <cosmic::Renderer as Headless>::new(Font::default(), Pixels(14.0), Some("tiny-skia")),
        )
        .expect("headless renderer");
        let item = crate::menu::MenuItem {
            label: "Open item".into(),
            message: (),
            icon: None,
            shortcut: None,
            mark: Some(MenuMark::Check(true)),
            disabled_reason: None,
            destructive: false,
        };
        fn icon(name: &'static str) -> widget::icon::Handle {
            widget::icon::from_name(name).handle()
        }
        fn body(text: String) -> Element<'static, ()> {
            widget::text(text).into()
        }
        let surface = MenuSurface::new(
            Menu::new().item(item),
            MenuStyle { icon, body },
            Anchors::default(),
            "test-menu".into(),
        );
        let mut panels = surface.panels(Id::new("source"), vec![], |_| (), |_| ());
        let mut stages = panels.remove(0).stages;
        let stage = stages.remove(0);
        let scrolling = stages.remove(0);
        assert_eq!(stage.content.presentation(), Some(Presentation::default()));
        let shadow = crate::popup_surface::shadow_padding();
        let minimum_width = stage
            .content
            .query(Level::Preferred, &renderer)
            .minimum
            .width;
        assert!(
            minimum_width > shadow.left + shadow.right + 24.0,
            "the native useful minimum includes its readable label and mark gutter"
        );
        assert!(
            minimum_width < 233.0,
            "this ordinary wrapped menu fits the narrow composer window including shadow"
        );
        assert!(
            stage
                .content
                .query_at(Level::Preferred, &renderer, Size::new(233.0, 300.0))
                .is_ok()
        );
        assert!(
            stage
                .content
                .query_at(
                    Level::Preferred,
                    &renderer,
                    Size::new(minimum_width - 1.0, 300.0)
                )
                .is_err()
        );
        let available = Size::new(minimum_width, 300.0);
        let expected = stage
            .content
            .query_at(Level::Preferred, &renderer, available)
            .unwrap();
        assert_eq!(expected.size.width, minimum_width);
        let inner = &expected.children[0];
        assert_eq!(
            inner.position,
            cosmic::iced::Point::new(shadow.left, shadow.top)
        );
        assert_eq!(
            inner.geometry.size.width,
            minimum_width - shadow.left - shadow.right
        );
        assert_eq!(
            expected.size.height,
            inner.geometry.size.height + shadow.top + shadow.bottom
        );
        let anchor = Rectangle::new(cosmic::iced::Point::new(0.0, 10.0), Size::new(20.0, 20.0));
        let window = Rectangle::with_size(Size::new(minimum_width, 300.0));
        let placed = layout::choose_attached(
            std::slice::from_ref(&stage),
            &renderer,
            window,
            &Attachment::below(anchor),
        )
        .unwrap();
        assert!(placed.bounds.x >= window.x);
        assert!(placed.bounds.y >= window.y);
        assert!(placed.bounds.x + placed.bounds.width <= window.width);
        assert!(placed.bounds.y + placed.bounds.height <= window.height);
        let mut root = crate::layout::stages(vec![stage]);
        let mut tree = Tree::empty();
        let node = root.layout(
            &mut tree,
            &renderer,
            &native_layout::Limits::new(Size::ZERO, available),
        );
        assert_eq!(
            root.selection().unwrap().as_ref().unwrap().geometry,
            expected
        );
        assert_eq!(node.children()[0].size(), expected.size);
        root.draw(
            &tree,
            &mut renderer,
            &cosmic::Theme::light(),
            &cosmic::iced::advanced::renderer::Style::default(),
            cosmic::iced::advanced::Layout::new(&node),
            cosmic::iced::advanced::mouse::Cursor::Unavailable,
            &Rectangle::with_size(available),
        );
        let minimum_height = 72.0 + shadow.top + shadow.bottom;
        assert_eq!(
            scrolling.content.query(Level::Mild, &renderer).minimum,
            Size::new(minimum_width, minimum_height)
        );
        assert!(
            scrolling
                .content
                .query_at(
                    Level::Mild,
                    &renderer,
                    Size::new(minimum_width, minimum_height - 1.0)
                )
                .is_err()
        );
        let available = Size::new(minimum_width, minimum_height);
        let expected = scrolling
            .content
            .query_at(Level::Mild, &renderer, available)
            .unwrap();
        let mut root = layout::stages(vec![scrolling]);
        let node = root.layout(
            &mut Tree::empty(),
            &renderer,
            &native_layout::Limits::new(Size::ZERO, available),
        );
        assert_eq!(
            root.selection().unwrap().as_ref().unwrap().geometry,
            expected
        );
        assert_eq!(node.children()[0].size(), available);
    }
    #[test]
    fn menu_action_activates_after_native_view_rebuilds() {
        use cosmic::iced::advanced::mouse;
        use cosmic::iced::advanced::{
            renderer::Headless,
            widget::{Operation, operation},
        };
        use cosmic::iced::runtime::{UserInterface, user_interface};
        use cosmic::iced::{Event, Font, Pixels};
        struct Find {
            id: Id,
            bounds: Option<Rectangle>,
        }
        impl Operation for Find {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                operate(self);
            }
            fn focusable(
                &mut self,
                id: Option<&Id>,
                bounds: Rectangle,
                _: &mut dyn operation::Focusable,
            ) {
                if id == Some(&self.id) {
                    self.bounds = Some(bounds);
                }
            }
        }
        fn icon(name: &'static str) -> widget::icon::Handle {
            widget::icon::from_name(name).handle()
        }
        fn body(text: String) -> Element<'static, bool> {
            widget::text(text).into()
        }
        fn view(anchors: &Anchors) -> crate::popup::Host<'static, bool> {
            let item = crate::menu::MenuItem {
                label: "Open item".into(),
                message: true,
                icon: None,
                shortcut: None,
                mark: None,
                disabled_reason: None,
                destructive: false,
            };
            let trigger = anchors.trigger("source", widget::space().width(40).height(20));
            let panels = MenuSurface::new(
                Menu::new().item(item),
                MenuStyle { icon, body },
                anchors.clone(),
                "test-menu".into(),
            )
            .panels(Id::new("source"), vec![], |_| false, |message| message);
            crate::popup::host(trigger)
                .anchors(anchors.clone())
                .chain(panels)
        }
        let mut renderer = cosmic::iced::futures::executor::block_on(
            <cosmic::Renderer as Headless>::new(Font::default(), Pixels(14.0), Some("tiny-skia")),
        )
        .expect("headless renderer");
        let anchors = Anchors::default();
        let size = Size::new(400.0, 300.0);
        let mut ui = UserInterface::build(
            view(&anchors),
            size,
            user_interface::Cache::new(),
            &mut renderer,
        );
        let mut find = Find {
            id: row_id("test-menu", &[], row_key("Open item", false, 0)),
            bounds: None,
        };
        ui.operate(&renderer, &mut find);
        let point = find.bounds.expect("native menu action").center();
        let mut cache = ui.into_cache();
        let mut messages = Vec::new();
        for event in [
            Event::Mouse(mouse::Event::CursorMoved { position: point }),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ] {
            let mut ui = UserInterface::build(view(&anchors), size, cache, &mut renderer);
            ui.update(
                &[event],
                mouse::Cursor::Available(point),
                &mut renderer,
                &mut cosmic::iced::advanced::clipboard::Null,
                &mut messages,
            );
            cache = ui.into_cache();
        }
        assert_eq!(messages, vec![true]);
    }
}

#[cfg(test)]
#[path = "menu_adaptive_tests.rs"]
mod adaptive_tests;
