// SPDX-License-Identifier: GPL-3.0-only

//! Native section tabs as one queryable child of a recursive layout.

use cosmic::iced::advanced::widget::{Operation, Tree, tree};
use cosmic::iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use cosmic::iced::{Event, Length, Rectangle, Size, Vector};
use cosmic::widget::segmented_button::{Entity, SingleSelectModel};
use cosmic::{Element, Renderer, Theme, widget};

use crate::layout::{Child, Level, child, measure_element};

const ICON_TEXT_GAP: u16 = 8;

#[derive(Clone)]
pub struct Tab {
    pub label: String,
    pub icon: widget::icon::Handle,
}

/// Persistent native models. Selection is a derived cache: call [`Model::select`]
/// whenever the application's canonical selected section changes.
pub struct Model {
    items: Vec<Tab>,
    names: Vec<String>,
    full: SingleSelectModel,
    compact: SingleSelectModel,
    full_ids: Vec<Entity>,
    compact_ids: Vec<Entity>,
    selected: usize,
    button_height: u16,
    font_size: f32,
    id: widget::segmented_button::Id,
}

impl Model {
    pub fn new(items: Vec<Tab>, selected: usize) -> Self {
        assert!(!items.is_empty(), "a tab strip has at least one section");
        let mut full = SingleSelectModel::default();
        let mut compact = SingleSelectModel::default();
        let mut full_ids = Vec::with_capacity(items.len());
        let mut compact_ids = Vec::with_capacity(items.len());
        for item in &items {
            full_ids.push(full.insert().text(item.label.clone()).id());
            compact_ids.push(
                compact
                    .insert()
                    .icon(widget::icon::icon(item.icon.clone()).size(16))
                    .id(),
            );
        }
        let names = items.iter().map(|item| item.label.clone()).collect();
        let mut model = Self {
            items,
            names,
            full,
            compact,
            full_ids,
            compact_ids,
            selected: 0,
            button_height: 44,
            font_size: 14.0,
            id: widget::segmented_button::Id::unique(),
        };
        model.select(selected);
        model
    }

    pub fn select(&mut self, selected: usize) {
        assert!(selected < self.items.len());
        self.selected = selected;
        self.full.activate(self.full_ids[selected]);
        self.compact.activate(self.compact_ids[selected]);
        for &id in &self.compact_ids {
            self.compact.text_remove(id);
        }
        self.compact.text_set(
            self.compact_ids[selected],
            self.items[selected].label.clone(),
        );
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn button_height(mut self, height: u16) -> Self {
        self.button_height = height;
        self
    }

    pub fn font_size(mut self, size: f32) -> Self {
        assert!(size.is_finite() && size > 0.0);
        self.font_size = size;
        self
    }

    fn ranges(&self, compact: bool, renderer: &Renderer) -> Vec<(f32, f32)> {
        let mut start = 0.0;
        let mut ranges = Vec::with_capacity(self.items.len());
        for (index, item) in self.items.iter().enumerate() {
            let mut single = SingleSelectModel::default();
            let id = single.insert().id();
            if !compact || index == self.selected {
                single.text_set(id, item.label.clone());
            }
            if compact {
                single.icon_set(id, widget::icon::icon(item.icon.clone()).size(16));
            }
            if index == self.selected {
                single.activate(id);
            }
            let width = measure_element(
                || {
                    native_bar(
                        &single,
                        vec![id],
                        |_| (),
                        self.button_height,
                        self.font_size,
                        widget::segmented_button::Id::unique(),
                    )
                },
                renderer,
            )
            .width;
            ranges.push((start, start + width));
            start += width;
        }
        ranges
    }

    fn selected_range(&self, renderer: &Renderer) -> (f32, f32) {
        self.ranges(false, renderer)[self.selected]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Overflow {
    /// Labels at 0/1, active label and icons at 2, named dropdown at 3.
    Dropdown,
    /// Full labels at 0; a horizontally scrolling viewport at 1/2/3.
    Scroll { minimum_viewport: f32 },
}

/// Native tab hover/selection styling, with the terminal's plain inactive
/// title colour on hover and press.
pub fn style() -> cosmic::theme::SegmentedButton {
    cosmic::theme::SegmentedButton::Custom(Box::new(|theme| {
        use widget::segmented_button::StyleSheet;
        let mut appearance = theme.horizontal(&cosmic::theme::SegmentedButton::TabBar);
        appearance.hover.text_color = appearance.inactive.text_color;
        appearance.pressed.text_color = appearance.inactive.text_color;
        appearance
    }))
}

fn native_bar<'a, Message: Clone + 'static>(
    model: &'a SingleSelectModel,
    ids: Vec<Entity>,
    on_select: fn(usize) -> Message,
    button_height: u16,
    font_size: f32,
    id: widget::segmented_button::Id,
) -> Element<'a, Message> {
    widget::tab_bar::horizontal(model)
        .id(id)
        .minimum_button_width(0)
        .maximum_button_width(u16::MAX)
        .button_height(button_height)
        .font_size(font_size)
        .button_spacing(ICON_TEXT_GAP)
        .spacing(0)
        .width(Length::Shrink)
        .style(style())
        .on_activate(move |entity| {
            on_select(
                ids.iter()
                    .position(|id| *id == entity)
                    .expect("native tab belongs to model"),
            )
        })
        .into()
}

/// A tab strip reports its own measurements; the enclosing parent chooses the
/// level and allocated box. It never restarts level selection internally.
pub fn strip<'a, Message: Clone + 'static>(
    model: &'a Model,
    overflow: Overflow,
    on_select: fn(usize) -> Message,
) -> Child<'a, Message> {
    if let Overflow::Scroll { minimum_viewport } = overflow {
        assert!(minimum_viewport.is_finite() && minimum_viewport >= 0.0);
    }
    child(move |level| match (overflow, level) {
        (Overflow::Dropdown, Level::AllPermitted) => widget::container(
            widget::dropdown(&model.names, Some(model.selected), on_select)
                .text_size(model.font_size),
        )
        .height(Length::Fixed(f32::from(model.button_height)))
        .into(),
        (Overflow::Dropdown, Level::Strong) => Element::new(NamedTabs {
            model,
            on_select,
            compact: true,
            content: widget::space().into(),
        }),
        (Overflow::Scroll { .. }, _) => {
            let content = widget::scrollable(native_bar(
                &model.full,
                model.full_ids.clone(),
                on_select,
                model.button_height,
                model.font_size,
                model.id.clone(),
            ))
            .horizontal()
            .width(Length::Fill)
            .height(Length::Fixed(f32::from(model.button_height)))
            .into();
            Element::new(Reveal { model, content })
        }
        _ => Element::new(NamedTabs {
            model,
            on_select,
            compact: false,
            content: widget::space().into(),
        }),
    })
    .minimum_width(move |level, preferred| match overflow {
        Overflow::Scroll { minimum_viewport } if level >= Level::Mild => {
            minimum_viewport.min(preferred.width)
        }
        _ => preferred.width,
    })
}

#[derive(Default)]
struct HoverState {
    hovered: Option<usize>,
    ranges: Vec<(f32, f32)>,
}

struct NamedTabs<'a, Message> {
    model: &'a Model,
    on_select: fn(usize) -> Message,
    compact: bool,
    content: Element<'a, Message>,
}

fn compact_with_tip<'a, Message: Clone + 'static>(
    model: &'a Model,
    hovered: Option<usize>,
    on_select: fn(usize) -> Message,
    compact: bool,
) -> Element<'a, Message> {
    let label = hovered
        .map(|index| model.items[index].label.clone())
        .unwrap_or_default();
    crate::tooltip_unless(
        native_bar(
            if compact { &model.compact } else { &model.full },
            if compact {
                model.compact_ids.clone()
            } else {
                model.full_ids.clone()
            },
            on_select,
            model.button_height,
            model.font_size,
            model.id.clone(),
        ),
        widget::text(label),
        widget::tooltip::Position::FollowCursor,
        !compact || hovered.is_none() || hovered == Some(model.selected),
    )
}

impl<Message: Clone + 'static> Widget<Message, Theme, Renderer> for NamedTabs<'_, Message> {
    forward_widget!(Message, Theme, Renderer; draw, operate, mouse_interaction, overlay, drag_destinations);
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<HoverState>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(HoverState::default())
    }
    fn diff(&mut self, _: &mut Tree) {}
    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<HoverState>();
        state.ranges = self.model.ranges(self.compact, renderer);
        self.content = compact_with_tip(self.model, state.hovered, self.on_select, self.compact);
        tree.diff_children(std::slice::from_mut(&mut self.content));
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<HoverState>();
        let hovered = cursor.position_over(layout.bounds()).and_then(|point| {
            state.ranges.iter().position(|(start, end)| {
                point.x - layout.bounds().x >= *start && point.x - layout.bounds().x < *end
            })
        });
        if state.hovered != hovered {
            state.hovered = hovered;
            self.content = compact_with_tip(self.model, hovered, self.on_select, self.compact);
            tree.diff_children(std::slice::from_mut(&mut self.content));
            shell.request_redraw();
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }
    #[cfg(feature = "a11y")]
    fn a11y_nodes(
        &self,
        layout: Layout<'_>,
        tree: &Tree,
        cursor: mouse::Cursor,
    ) -> iced_accessibility::A11yTree {
        self.content
            .as_widget()
            .a11y_nodes(layout, &tree.children[0], cursor)
    }
}

#[derive(Default)]
struct RevealState {
    last: Option<(usize, u32, u32, u32)>,
}

struct Reveal<'a, Message> {
    model: &'a Model,
    content: Element<'a, Message>,
}

impl<Message> Widget<Message, Theme, Renderer> for Reveal<'_, Message> {
    forward_widget!(Message, Theme, Renderer; children, diff, size, draw, update, operate, mouse_interaction, overlay, drag_destinations);

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<RevealState>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(RevealState::default())
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let node = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        let (start, end) = self.model.selected_range(renderer);
        let key = (
            self.model.selected,
            node.size().width.to_bits(),
            start.to_bits(),
            end.to_bits(),
        );
        let state = tree.state.downcast_mut::<RevealState>();
        if state.last != Some(key) {
            state.last = Some(key);
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                Layout::new(&node),
                renderer,
                &mut RevealSelection { start, end },
            );
        }
        node
    }

    #[cfg(feature = "a11y")]
    fn a11y_nodes(
        &self,
        layout: Layout<'_>,
        tree: &Tree,
        cursor: mouse::Cursor,
    ) -> iced_accessibility::A11yTree {
        self.content
            .as_widget()
            .a11y_nodes(layout, &tree.children[0], cursor)
    }
}

struct RevealSelection {
    start: f32,
    end: f32,
}
impl Operation for RevealSelection {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn scrollable(
        &mut self,
        _id: Option<&cosmic::iced::advanced::widget::Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        state: &mut dyn cosmic::iced::advanced::widget::operation::Scrollable,
    ) {
        let offset = if self.start < translation.x {
            self.start
        } else if self.end > translation.x + bounds.width {
            (self.end - bounds.width).min(self.start)
        } else {
            translation.x
        };
        state.scroll_to(
            cosmic::iced::advanced::widget::operation::scrollable::AbsoluteOffset {
                x: Some(offset.clamp(0.0, (content.width - bounds.width).max(0.0))),
                y: None,
            },
        );
    }
}

#[cfg(test)]
#[path = "tabs_tests.rs"]
mod tests;
