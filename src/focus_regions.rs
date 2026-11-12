// SPDX-License-Identifier: GPL-3.0-only
//! Keyboard movement among an editor and ordered groups of controls.
//!
//! The native tree determines which configured controls exist after responsive
//! layout. Navigation itself is shared with other lists; this wrapper only
//! maps its results to native focus operations.
use crate::list_navigation::{Axis, Direction, Entry, Navigation};
use cosmic::iced::advanced::widget::{Id, Operation, Tree, operation, tree};
use cosmic::iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use cosmic::iced::{Event, Length, Rectangle, Size, Vector, keyboard};
use cosmic::{Element, Renderer, Theme};

pub struct Group {
    pub axis: Axis,
    pub ids: Vec<Id>,
}
impl Group {
    pub fn new(axis: Axis, ids: Vec<Id>) -> Self {
        Self { axis, ids }
    }
}

pub fn regions<'a, M: Clone + 'a>(
    content: impl Into<Element<'a, M>>,
    editor: Id,
    groups: Vec<Group>,
    anchors: crate::popup::Anchors,
) -> Element<'a, M> {
    Element::new(Regions {
        content: content.into(),
        editor,
        groups,
        anchors,
    })
}

struct Regions<'a, M> {
    content: Element<'a, M>,
    editor: Id,
    groups: Vec<Group>,
    anchors: crate::popup::Anchors,
}
#[derive(Default)]
struct State {
    groups: Vec<Navigation<Id>>,
    group: Option<Navigation<usize>>,
    owned: Option<Id>,
}
#[derive(Default)]
struct Probe {
    present: Vec<Id>,
    focused: Option<Id>,
    any_focused: bool,
}
impl Operation for Probe {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn focusable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        state: &mut dyn operation::Focusable,
    ) {
        if state.is_focused() {
            self.any_focused = true;
        }
        if let Some(id) = id.filter(|_| bounds.width > 0.0 && bounds.height > 0.0) {
            self.present.push(id.clone());
            if state.is_focused() {
                self.focused = Some(id.clone());
            }
        }
    }
}
impl<'a, M: Clone + 'a> Widget<M, Theme, Renderer> for Regions<'a, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    forward_widget!(M, Theme, Renderer; children, diff, size, layout, draw, operate, mouse_interaction, overlay, drag_destinations);
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let mut probe = Probe::default();
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, &mut probe);
        let state = tree.state.downcast_mut::<State>();
        state
            .groups
            .resize_with(self.groups.len(), || Navigation::new(Axis::Horizontal));
        state.groups.truncate(self.groups.len());
        let mut group_entries = Vec::new();
        for (index, spec) in self.groups.iter().enumerate() {
            if state.groups[index].axis() != spec.axis {
                state.groups[index] = Navigation::new(spec.axis);
            }
            let entries: Vec<_> = spec
                .ids
                .iter()
                .filter(|id| probe.present.contains(id))
                .cloned()
                .map(|id| Entry::new(id, true))
                .collect();
            state.groups[index].reconcile(&entries);
            if !entries.is_empty() {
                group_entries.push(Entry::new(index, true));
            }
            if let Some(focused) = &probe.focused {
                state.groups[index].set_current(focused);
            }
        }
        let group = state
            .group
            .get_or_insert_with(|| Navigation::new(Axis::Horizontal));
        group.reconcile(&group_entries);
        let mut focused_group = probe
            .focused
            .as_ref()
            .and_then(|id| self.groups.iter().position(|group| group.ids.contains(id)));
        if let Some(index) = focused_group {
            group.set_current(&index);
            state.owned = probe.focused.clone();
        } else if probe.any_focused {
            state.owned = None;
        } else if self.anchors.surfaces().is_empty()
            && let Some(previous) = state.owned.clone()
        {
            if !probe.present.contains(&previous) {
                // Responsive forms may replace a whole group with one
                // control. Repair focus only when no other native target
                // owns it and the old target genuinely disappeared.
                if let Some(index) = group.current().copied()
                    && let Some(id) = state.groups[index]
                        .current()
                        .cloned()
                        .or_else(|| state.groups[index].navigate(Direction::First).cloned())
                {
                    let mut focus = operation::focusable::focus::<()>(id.clone());
                    self.content.as_widget_mut().operate(
                        &mut tree.children[0],
                        layout,
                        renderer,
                        &mut focus,
                    );
                    let mut show = crate::popup::show_keyboard_target(id.clone(), true);
                    self.content.as_widget_mut().operate(
                        &mut tree.children[0],
                        layout,
                        renderer,
                        &mut show,
                    );
                    self.anchors.keyboard_target(&id);
                    state.owned = Some(id.clone());
                    probe.focused = Some(id);
                    focused_group = Some(index);
                    shell.request_redraw();
                }
            } else {
                // The old control remains present but was unfocused by
                // another region; never claim focus back from that region.
                state.owned = None;
            }
        }
        let key = match event {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(key),
                modifiers,
                ..
            }) => Some((*key, *modifiers)),
            _ => None,
        };
        use keyboard::key::Named;
        let mut destination = None;
        if let Some((Named::F6, modifiers)) =
            key.filter(|(_, m)| !m.control() && !m.alt() && !m.logo())
        {
            if probe.focused.as_ref() == Some(&self.editor) {
                let index = group.current().copied().or_else(|| {
                    group
                        .navigate(if modifiers.shift() {
                            Direction::Last
                        } else {
                            Direction::First
                        })
                        .copied()
                });
                if let Some(index) = index {
                    destination = state.groups[index].current().cloned().or_else(|| {
                        state.groups[index]
                            .navigate(if modifiers.shift() {
                                Direction::Last
                            } else {
                                Direction::First
                            })
                            .cloned()
                    });
                }
            } else if focused_group.is_some() {
                destination = Some(self.editor.clone());
            }
        }
        if destination.is_none() {
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
            if shell.is_event_captured() {
                return;
            }
            if let (Some(index), Some((key, modifiers))) = (focused_group, key)
                && !modifiers.control()
                && !modifiers.alt()
                && !modifiers.logo()
            {
                destination = match key {
                    Named::Tab => group
                        .navigate(if modifiers.shift() {
                            Direction::Previous
                        } else {
                            Direction::Next
                        })
                        .copied()
                        .and_then(|i| {
                            state.groups[i]
                                .current()
                                .cloned()
                                .or_else(|| state.groups[i].navigate(Direction::First).cloned())
                        }),
                    Named::ArrowLeft
                        if !modifiers.shift() && self.groups[index].axis == Axis::Horizontal =>
                    {
                        state.groups[index].navigate(Direction::Previous).cloned()
                    }
                    Named::ArrowRight
                        if !modifiers.shift() && self.groups[index].axis == Axis::Horizontal =>
                    {
                        state.groups[index].navigate(Direction::Next).cloned()
                    }
                    Named::ArrowUp
                        if !modifiers.shift() && self.groups[index].axis == Axis::Vertical =>
                    {
                        state.groups[index].navigate(Direction::Previous).cloned()
                    }
                    Named::ArrowDown
                        if !modifiers.shift() && self.groups[index].axis == Axis::Vertical =>
                    {
                        state.groups[index].navigate(Direction::Next).cloned()
                    }
                    Named::Home if !modifiers.shift() => {
                        state.groups[index].navigate(Direction::First).cloned()
                    }
                    Named::End if !modifiers.shift() => {
                        state.groups[index].navigate(Direction::Last).cloned()
                    }
                    _ => None,
                };
            }
        }
        if let Some(id) = destination {
            state.owned = (id != self.editor).then(|| id.clone());
            if let Some(previous) = probe.focused {
                let mut hide = crate::popup::show_keyboard_target(previous, false);
                self.content.as_widget_mut().operate(
                    &mut tree.children[0],
                    layout,
                    renderer,
                    &mut hide,
                );
            }
            let mut focus = operation::focusable::focus::<()>(id.clone());
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                &mut focus,
            );
            if id != self.editor {
                let mut show = crate::popup::show_keyboard_target(id.clone(), true);
                self.content.as_widget_mut().operate(
                    &mut tree.children[0],
                    layout,
                    renderer,
                    &mut show,
                );
                self.anchors.keyboard_target(&id);
            } else {
                self.anchors.keyboard_target(&id);
            }
            shell.capture_event();
            shell.request_redraw();
        }
    }
}
