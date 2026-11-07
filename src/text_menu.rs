// SPDX-License-Identifier: GPL-3.0-only

//! Household text menus. Native targets retain focus, selection and editing;
//! this wrapper supplies the household vocabulary, grouping and menu rows.

use std::cell::RefCell;

use cosmic::iced::advanced::layout::{self, Layout};
use cosmic::iced::advanced::renderer;
use cosmic::iced::advanced::widget::{Operation, Tree, Widget, tree};
use cosmic::iced::advanced::{Clipboard, Shell, mouse, overlay};
use cosmic::iced::keyboard::Key;
use cosmic::iced::{Event, Length, Rectangle, Size, Vector, window};
use cosmic::widget::menu::key_bind::{KeyBind, Modifier};
use cosmic::widget::text_context_menu::{
    TextAction, TextKind, TextMenuCommand, TextMenuContext, TextMenuGeneration, TextMenuSession,
    TextMenuTarget, TextMenuView,
};
use cosmic::{Element, Renderer, Theme};

use crate::menu::{Menu, MenuItem, MenuStyle};

/// Localized labels and disabled explanations, supplied by the application.
#[derive(Clone, Debug)]
pub struct TextMenuWords {
    pub cut: String,
    pub copy: String,
    pub paste: String,
    pub select_all: String,
    pub no_selection: String,
    pub hidden: String,
    pub read_only: String,
    pub clipboard_empty: String,
    pub empty_content: String,
    /// A text box's Undo and Redo ([`TextMenu::history`]), and why each is
    /// disabled with nothing to undo or redo
    pub undo: String,
    pub redo: String,
    pub nothing_to_undo: String,
    pub nothing_to_redo: String,
}

/// A text box's undo history, as its menu offers it: Undo and Redo after
/// Select all, each publishing the box's own message (Juliet approved, 7
/// Nov 2026). Made from a [`TextBox`](crate::TextBox) by [`Self::of`]
#[derive(Clone)]
pub struct TextMenuHistory<M> {
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo: M,
    pub redo: M,
}

impl<M> TextMenuHistory<M> {
    /// `text_box`'s history, `on_action` its message, as its editor's
    pub fn of<P: Clone>(
        text_box: &crate::TextBox<P>,
        on_action: impl Fn(crate::text_box::Action) -> M,
    ) -> Self {
        Self {
            can_undo: text_box.can_undo(),
            can_redo: text_box.can_redo(),
            undo: on_action(crate::text_box::Action::Undo),
            redo: on_action(crate::text_box::Action::Redo),
        }
    }
}

/// A concrete native text target with the household's text menu.
pub struct TextMenu<W, M> {
    content: W,
    words: TextMenuWords,
    actions: Menu<M>,
    style: MenuStyle<TextMenuCommand>,
    window: window::Id,
    read_only: bool,
    history: Option<TextMenuHistory<M>>,
}

/// Configure a native target before converting it into an element.
pub fn text_menu<W, M>(
    mut target: W,
    words: &TextMenuWords,
    style: MenuStyle<TextMenuCommand>,
) -> TextMenu<W, M>
where
    W: TextMenuTarget<M>,
{
    target.use_external_text_menu();
    TextMenu {
        content: target,
        words: words.clone(),
        actions: Menu::new(),
        style,
        window: cosmic::widget::text_context_menu::current_window_id(),
        read_only: false,
        history: None,
    }
}

impl<W, M> TextMenu<W, M> {
    /// Custom actions are snapshotted when the menu opens.
    pub fn actions(mut self, actions: Menu<M>) -> Self {
        self.actions = actions;
        self
    }

    /// A text box's Undo and Redo, after Select all; snapshotted when the
    /// menu opens, as the actions are
    pub fn history(mut self, history: TextMenuHistory<M>) -> Self {
        self.history = Some(history);
        self
    }

    /// Further restrict an editable target without changing its native callbacks.
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    /// Override the window captured when the widget was constructed.
    pub fn window_id(mut self, window: window::Id) -> Self {
        self.window = window;
        self
    }
}

/// The menu for a target with `context`: its rows, and the messages its
/// custom rows (`actions`, `history`) publish by index. Public for an app's
/// tests of what its menus hold
pub fn resolved_menu<M>(
    context: TextMenuContext,
    words: &TextMenuWords,
    actions: Menu<M>,
    history: Option<TextMenuHistory<M>>,
    icon: fn(&'static str) -> cosmic::widget::icon::Handle,
) -> (Menu<TextMenuCommand>, Vec<M>) {
    let messages = RefCell::new(Vec::new());
    let custom = actions.map(|message| {
        let mut messages = messages.borrow_mut();
        let index = messages.len();
        messages.push(message);
        TextMenuCommand::Custom(index)
    });
    let field = context.kind != TextKind::Selectable;
    let read_only = context.kind != TextKind::Editable;
    let reason = |action| -> Option<String> {
        match action {
            TextAction::Cut if read_only => Some(words.read_only.clone()),
            TextAction::Cut | TextAction::Copy if context.secure => Some(words.hidden.clone()),
            TextAction::Cut | TextAction::Copy if !context.has_selection => {
                Some(words.no_selection.clone())
            }
            TextAction::Paste if read_only => Some(words.read_only.clone()),
            TextAction::Paste if !context.clipboard_has_text => Some(words.clipboard_empty.clone()),
            TextAction::SelectAll if !context.has_text => Some(words.empty_content.clone()),
            _ => None,
        }
    };
    let item =
        |action, label: &String, name: Option<&'static str>, letter: &'static str| MenuItem {
            label: label.clone(),
            message: TextMenuCommand::Edit(action),
            icon: name.map(icon),
            shortcut: Some(KeyBind {
                modifiers: vec![Modifier::Ctrl],
                key: Key::Character(letter.into()),
            }),
            mark: None,
            disabled_reason: reason(action),
            destructive: false,
        };
    let mut text = Menu::new();
    let cut = field.then(|| item(TextAction::Cut, &words.cut, Some("edit-cut-symbolic"), "x"));
    let copy = item(
        TextAction::Copy,
        &words.copy,
        Some("edit-copy-symbolic"),
        "c",
    );
    // With text selected, Copy first (household rule 28: a menu for a
    // selection starts with Copy)
    if context.has_selection {
        text = text.item(copy);
        if let Some(cut) = cut {
            text = text.item(cut);
        }
    } else {
        if let Some(cut) = cut {
            text = text.item(cut);
        }
        text = text.item(copy);
    }
    if field {
        text = text.item(item(
            TextAction::Paste,
            &words.paste,
            Some("edit-paste-symbolic"),
            "v",
        ));
    }
    text = text.item(item(TextAction::SelectAll, &words.select_all, None, "a"));
    // A text box's Undo · Redo, a group of their own after Select all
    if let Some(history) = history.filter(|_| field) {
        let mut messages = messages.borrow_mut();
        let mut custom = |message| {
            let index = messages.len();
            messages.push(message);
            TextMenuCommand::Custom(index)
        };
        let key = |wanted: crate::text_box::Action| {
            crate::text_box::standard_keys()
                .into_iter()
                .find(|(_, action)| *action == wanted)
                .map(|(bind, _)| bind)
        };
        let reason = |can: bool, nothing: &String| {
            if read_only {
                Some(words.read_only.clone())
            } else {
                (!can).then(|| nothing.clone())
            }
        };
        let row = |label: &String, message, action, reason| MenuItem {
            label: label.clone(),
            message,
            icon: None,
            shortcut: key(action),
            mark: None,
            disabled_reason: reason,
            destructive: false,
        };
        let undo = row(
            &words.undo,
            custom(history.undo),
            crate::text_box::Action::Undo,
            reason(history.can_undo, &words.nothing_to_undo),
        );
        let redo = row(
            &words.redo,
            custom(history.redo),
            crate::text_box::Action::Redo,
            reason(history.can_redo, &words.nothing_to_redo),
        );
        text = text.append(Menu::new().item(undo).item(redo));
    }
    let menu = if context.has_selection {
        text.append(custom)
    } else {
        custom.append(text)
    };
    (menu, messages.into_inner())
}

struct State<M> {
    session: TextMenuSession,
    snapshot: Option<(TextMenuGeneration, Vec<M>)>,
}

impl<M> Default for State<M> {
    fn default() -> Self {
        Self {
            session: TextMenuSession::default(),
            snapshot: None,
        }
    }
}

impl<W, M> Widget<M, Theme, Renderer> for TextMenu<W, M>
where
    W: Widget<M, Theme, Renderer> + TextMenuTarget<M>,
    M: Clone + 'static,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<M>>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::<M>::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content as &dyn Widget<M, Theme, Renderer>)]
    }
    fn diff(&mut self, tree: &mut Tree) {
        if tree.children[0].tag != self.content.tag() {
            let state = tree.state.downcast_mut::<State<M>>();
            state.session.dismiss();
            state.snapshot = None;
        }
        tree.children[0].diff(&mut self.content as &mut dyn Widget<M, Theme, Renderer>);
    }
    fn size(&self) -> Size<Length> {
        self.content.size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.size_hint()
    }
    // The named child already carries its own identity. Giving the wrapper
    // the same ID would make named-state extraction confuse their states.
    fn id(&self) -> Option<cosmic::iced::advanced::widget::Id> {
        None
    }
    fn set_id(&mut self, id: cosmic::iced::advanced::widget::Id) {
        self.content.set_id(id);
    }
    #[cfg(feature = "a11y")]
    fn a11y_nodes(
        &self,
        layout: Layout<'_>,
        tree: &Tree,
        cursor: mouse::Cursor,
    ) -> iced_accessibility::A11yTree {
        self.content.a11y_nodes(layout, &tree.children[0], cursor)
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.layout(&mut tree.children[0], renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
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
        let state = tree.state.downcast_mut::<State<M>>();
        if let Some(choice) = state.session.take_choice()
            && let Some((generation, messages)) = state.snapshot.take()
        {
            if choice.generation == generation {
                match choice.command {
                    TextMenuCommand::Edit(action) => {
                        if !self.read_only || !matches!(action, TextAction::Cut | TextAction::Paste)
                        {
                            self.content.execute_text_menu_action(
                                &mut tree.children[0],
                                action,
                                clipboard,
                                shell,
                            );
                        }
                    }
                    TextMenuCommand::Custom(index) => {
                        if let Some(message) = messages.get(index) {
                            shell.publish(message.clone());
                        }
                    }
                }
            } else {
                state.snapshot = Some((generation, messages));
            }
        }
        if state.session.dismiss_on_event(event) {
            state.snapshot = None;
        }
        self.content.update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if !state.session.is_open() {
            state.snapshot = None;
        }
        if let Some(at) = self.content.take_text_menu_request(&mut tree.children[0]) {
            let mut context = self.content.text_menu_context(&tree.children[0]);
            if self.read_only && context.kind == TextKind::Editable {
                context.kind = TextKind::ReadOnlyInput;
            }
            let (menu, messages) = resolved_menu(
                context,
                &self.words,
                self.actions.clone(),
                self.history.clone(),
                self.style.icon,
            );
            let width = menu.width();
            let roots = vec![cosmic::widget::menu::Tree::with_children(
                Element::from(cosmic::widget::Row::new()),
                menu.trees(self.style),
            )];
            let generation = state.session.open(
                TextMenuView {
                    roots,
                    item_width: cosmic::widget::menu::ItemWidth::Uniform(width),
                },
                at,
                renderer,
                viewport,
                cursor,
                self.window,
            );
            state.snapshot = Some((generation, messages));
        }
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        let state = tree.state.downcast_mut::<State<M>>();
        let menu = state.session.overlay(translation);
        let child = self.content.overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        );
        match (child, menu) {
            (Some(child), Some(menu)) => {
                Some(overlay::Group::with_children(vec![child, menu]).overlay())
            }
            (Some(child), None) => Some(child),
            (None, Some(menu)) => Some(menu),
            (None, None) => None,
        }
    }
    fn drag_destinations(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        destinations: &mut cosmic::iced::advanced::clipboard::DndDestinationRectangles,
    ) {
        self.content
            .drag_destinations(&tree.children[0], layout, renderer, destinations);
    }
}

impl<'a, W, M> From<TextMenu<W, M>> for Element<'a, M>
where
    W: Widget<M, Theme, Renderer> + TextMenuTarget<M> + 'a,
    M: Clone + 'static,
{
    fn from(menu: TextMenu<W, M>) -> Self {
        Element::new(menu)
    }
}

#[cfg(test)]
#[path = "text_menu_tests.rs"]
mod tests;
