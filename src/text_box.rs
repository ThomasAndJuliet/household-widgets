// SPDX-License-Identifier: GPL-3.0-only

//! A text box that does what a text box does. Juliet, 6 Nov 2026, of COSMIC
//! Terminal's prompt: "You were asked to implement a input text box. This
//! obviously includes the usual functionality of such a box which includes
//! ctrl + z."
//!
//! iced's `text_editor` is the box; this adds what it lacks, without
//! changing libcosmic:
//!
//! - [`TextBox`]: its text and caret, as iced's `Content` (which it derefs
//!   to, for `text_editor(&text_box)`, `.text()`, `.selection()`), with an
//!   undo history. Every change goes through [`TextBox::perform`] or
//!   [`TextBox::replace`], so every change can be undone.
//! - [`Action`]: what the box is asked to do: iced's own actions, and
//!   Undo, Redo, deleting a word, and Shift+click.
//! - [`key_binding`]: the standard keys (household rule 21c). An app with
//!   keys of its own handles those first and ends its binding with this.
//! - [`shift_click`]: round the box, so that Shift+click extends the
//!   selection.
//!
//! ```ignore
//! let editor = text_editor(&self.draft)
//!     .on_action(Message::Draft)        // via `Action::from`, below
//!     .key_binding(|press| text_box::key_binding(press, Message::Draft));
//! text_box::shift_click(editor, Message::Draft(text_box::Action::Extend))
//! // and in update: Message::Draft(action) => self.draft.perform(action)
//! ```
//!
//! with `on_action(|action| Message::Draft(action.into()))` where iced's
//! actions come in.
//!
//! **Steps.** Ctrl+Z goes back one step, as in GTK and the browsers: a run
//! of typing is one step, ended by moving the caret (a key or a click), by
//! changing from typing to deleting or back, and by a space or new line
//! typed after a word, which starts the next. A paste, a cut, deleting a
//! selection, deleting a word and every change the app makes
//! ([`TextBox::replace`]) are a step each. Typing over a selection is one
//! step with the typing that follows it. New typing after Ctrl+Z drops
//! what Ctrl+Shift+Z would have brought back. The history keeps the last
//! [`STEPS`] steps. Undo puts back the caret and selection as they were.
//!
//! **Keys** ([`key_binding`]), added to iced's: Ctrl+Z undoes; Ctrl+Shift+Z
//! and Ctrl+Y redo; Ctrl+Backspace and Ctrl+Delete delete a word, or the
//! selection. iced already has Ctrl+A; Ctrl+X, C and V; Shift+Delete,
//! Ctrl+Insert and Shift+Insert; Ctrl+Left and Right by words; Home, End,
//! Ctrl+Home and Ctrl+End; with Shift, each of those moves selects; and a
//! double click selects a word, a triple click a line. A key is the box's
//! only while it is focused: unfocused, the binding gives nothing, so the
//! key is not captured and goes on to the app (an app's own Ctrl+Z, the
//! terminal's ^Z). Focused, a key the box takes is captured.
//!
//! **One-line boxes** (`text_input`) will get the same through a history
//! over the value the app keeps (`String` states in the same history, with
//! the same steps, told apart by comparing each new value with the last),
//! and a wrapper like [`shift_click`]'s that takes the undo keys while the
//! input it wraps is focused. A password box keeps no history, as in GTK.

use std::collections::VecDeque;
use std::ops::Deref;

use cosmic::Element;
use cosmic::iced::advanced::layout::{self, Layout};
use cosmic::iced::advanced::renderer;
use cosmic::iced::advanced::widget::{Operation, Tree, Widget, tree};
use cosmic::iced::advanced::{Clipboard, Shell, mouse, overlay};
use cosmic::iced::keyboard::{self, Key, Modifiers, key::Named};
use cosmic::iced::{Event, Length, Rectangle, Size, Vector};
use cosmic::widget::text_editor::{self, Binding, Cursor, Edit, KeyPress, Motion, Position};

/// The steps Ctrl+Z can go back
pub const STEPS: usize = 300;

/// iced's text and caret, which a [`TextBox`] holds
pub type Content = text_editor::Content<cosmic::Renderer>;

/// What a [`TextBox`] is asked to do
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// iced's own: a move, a selection, a click, an edit
    Editor(text_editor::Action),
    /// Back one step (Ctrl+Z)
    Undo,
    /// Forward one step (Ctrl+Shift+Z, Ctrl+Y)
    Redo,
    /// The word before the caret deleted, or the selection (Ctrl+Backspace)
    DeleteWordBefore,
    /// The word after the caret deleted, or the selection (Ctrl+Delete)
    DeleteWordAfter,
    /// Shift held as the box is clicked: the click that follows extends the
    /// selection rather than placing the caret ([`shift_click`])
    Extend,
}

impl Action {
    /// Whether it changes the text, or may: a box that cannot be changed
    /// does nothing of these
    pub fn is_edit(&self) -> bool {
        match self {
            Self::Editor(action) => action.is_edit(),
            Self::Undo | Self::Redo | Self::DeleteWordBefore | Self::DeleteWordAfter => true,
            Self::Extend => false,
        }
    }
}

impl From<text_editor::Action> for Action {
    fn from(action: text_editor::Action) -> Self {
        Self::Editor(action)
    }
}

/// The steps a box can go back and forward through: each the state before
/// it (in `undo`) or after it (in `redo`). `S` is a box's whole state, so
/// that going back is putting it back: a text editor's text and caret, a
/// one-line box's value.
#[derive(Debug)]
struct History<S> {
    undo: VecDeque<S>,
    redo: Vec<S>,
}

impl<S> Default for History<S> {
    fn default() -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
        }
    }
}

impl<S> History<S> {
    /// A step taken from `before`: what Redo would have brought back is gone
    fn record(&mut self, before: S) {
        self.undo.push_back(before);
        if self.undo.len() > STEPS {
            self.undo.pop_front();
        }
        self.redo.clear();
    }

    /// The state before the last step, `now` kept for Redo
    fn undo(&mut self, now: S) -> Option<S> {
        let before = self.undo.pop_back()?;
        self.redo.push(now);
        Some(before)
    }

    /// The state after the step last undone, `now` kept for Undo
    fn redo(&mut self, now: S) -> Option<S> {
        let after = self.redo.pop()?;
        self.undo.push_back(now);
        Some(after)
    }
}

/// The run of typing or deleting going on, which the next edit of the same
/// kind joins as part of the same step
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Run {
    /// Typing; whether the last character was a space or new line
    Typing {
        space: bool,
    },
    Deleting,
}

impl Run {
    /// Whether `next`, after this, is part of the same step: typing goes on
    /// until a space after a word starts the next step
    fn joined_by(self, next: Run) -> bool {
        match (self, next) {
            (Run::Typing { space }, Run::Typing { space: next }) => space || !next,
            (Run::Deleting, Run::Deleting) => true,
            _ => false,
        }
    }
}

/// A text editor's whole state, to go back to
#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    text: String,
    cursor: Cursor,
}

/// A text box's text and caret, with its undo history; see the module's
/// description. It derefs to iced's [`Content`], for the editor widget and
/// for reading; changes go through [`perform`](Self::perform) and
/// [`replace`](Self::replace).
#[derive(Default)]
pub struct TextBox {
    content: Content,
    history: History<Snapshot>,
    /// The run of typing or deleting going on, if any
    run: Option<Run>,
    /// Where a Shift+click extends the selection from: the anchor when the
    /// press was announced ([`Action::Extend`])
    anchor: Option<Position>,
}

impl Deref for TextBox {
    type Target = Content;

    fn deref(&self) -> &Content {
        &self.content
    }
}

impl std::fmt::Debug for TextBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextBox")
            .field("text", &self.content.text())
            .field("cursor", &self.content.cursor())
            .field("undo", &self.history.undo.len())
            .field("redo", &self.history.redo.len())
            .finish()
    }
}

impl TextBox {
    /// An empty box, with nothing to undo
    pub fn new() -> Self {
        Self::default()
    }

    /// A box holding `text`, the caret at its start (as iced's
    /// `Content::with_text`), with nothing to undo
    pub fn with_text(text: &str) -> Self {
        Self {
            content: Content::with_text(text),
            ..Self::default()
        }
    }

    /// Whether Undo would change anything
    pub fn can_undo(&self) -> bool {
        !self.history.undo.is_empty()
    }

    /// Whether Redo would change anything
    pub fn can_redo(&self) -> bool {
        !self.history.redo.is_empty()
    }

    /// The text replaced by the app, the caret at its end: one step, which
    /// Undo takes back
    pub fn replace(&mut self, text: &str) {
        self.change(|content| {
            *content = Content::with_text(text);
            content.perform(text_editor::Action::Move(Motion::DocumentEnd));
        });
        // Even when the text is the same, the caret is at its end
        self.content
            .perform(text_editor::Action::Move(Motion::DocumentEnd));
        self.run = None;
    }

    /// Does `action`, keeping the history
    pub fn perform(&mut self, action: impl Into<Action>) {
        let anchor = self.anchor.take();
        match action.into() {
            Action::Undo => {
                if self.can_undo() {
                    let now = self.snapshot();
                    let before = self.history.undo(now).expect("a step to undo");
                    self.restore(before);
                }
            }
            Action::Redo => {
                if self.can_redo() {
                    let now = self.snapshot();
                    let after = self.history.redo(now).expect("a step to redo");
                    self.restore(after);
                }
            }
            Action::DeleteWordBefore => self.delete_word(Motion::WordLeft, Edit::Backspace),
            Action::DeleteWordAfter => self.delete_word(Motion::WordRight, Edit::Delete),
            Action::Extend => {
                let cursor = self.content.cursor();
                self.anchor = Some(cursor.selection.unwrap_or(cursor.position));
            }
            Action::Editor(text_editor::Action::Edit(edit)) => self.edit(edit),
            // A scroll leaves the caret where it is
            Action::Editor(scroll @ text_editor::Action::Scroll { .. }) => {
                self.content.perform(scroll);
            }
            Action::Editor(action) => {
                self.run = None;
                let click = matches!(action, text_editor::Action::Click(_));
                self.content.perform(action);
                // Shift+click: from the anchor to where the click put the
                // caret
                if click && let Some(anchor) = anchor {
                    let position = self.content.cursor().position;
                    self.content.move_to(Cursor {
                        position,
                        selection: Some(anchor),
                    });
                }
            }
        }
    }

    /// An edit of iced's: part of the run going on, or a step of its own
    fn edit(&mut self, edit: Edit) {
        let selected = self.content.selection().is_some();
        let run = match edit {
            Edit::Insert(c) => Some(Run::Typing {
                space: c.is_whitespace(),
            }),
            Edit::Enter => Some(Run::Typing { space: true }),
            // Deleting a selection is a step of its own
            Edit::Backspace | Edit::Delete if !selected => Some(Run::Deleting),
            // A paste, an indent
            _ => None,
        };
        let joins = !selected
            && match (self.run, run) {
                (Some(going), Some(next)) => going.joined_by(next),
                _ => false,
            };
        let action = text_editor::Action::Edit(edit);
        let changed = if joins {
            self.content.perform(action);
            true
        } else {
            self.change(|content| content.perform(action))
        };
        // A key that changed nothing (Backspace at the start) is no run:
        // the next edit is not part of the step before
        self.run = run.filter(|_| changed);
    }

    /// A word deleted towards `motion`, or the selection if there is one:
    /// a step of its own
    fn delete_word(&mut self, motion: Motion, edit: Edit) {
        self.run = None;
        let selected = self.content.selection().is_some();
        self.change(|content| {
            if !selected {
                content.perform(text_editor::Action::Select(motion));
            }
            content.perform(text_editor::Action::Edit(edit));
        });
    }

    /// `change` made as a step of its own, if it changes the text; whether
    /// it did
    fn change(&mut self, change: impl FnOnce(&mut Content)) -> bool {
        let before = self.snapshot();
        change(&mut self.content);
        let changed = self.content.text() != before.text;
        if changed {
            self.history.record(before);
        }
        changed
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.content.text(),
            cursor: self.content.cursor(),
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.content = Content::with_text(&snapshot.text);
        self.content.move_to(snapshot.cursor);
        self.run = None;
    }
}

/// What a key does to a box's history, as household rule 21c has it:
/// Ctrl+Z undoes; Ctrl+Shift+Z, and Ctrl+Y as Cerea and Marginalium also
/// have it, redo. Any keyboard layout: by the key's Latin letter
fn history_key(
    key: &Key,
    physical: keyboard::key::Physical,
    modifiers: Modifiers,
) -> Option<Action> {
    if !modifiers.command() || modifiers.alt() || modifiers.logo() {
        return None;
    }
    match key.to_latin(physical).map(|c| c.to_ascii_lowercase()) {
        Some('z') if modifiers.shift() => Some(Action::Redo),
        Some('z') => Some(Action::Undo),
        Some('y') if !modifiers.shift() => Some(Action::Redo),
        _ => None,
    }
}

/// The standard keys of a text box (see the module's description), for
/// `text_editor(..).key_binding(..)`: iced's, with Undo, Redo and deleting
/// a word added. `on_action` is the box's message, as given to
/// `on_action`. An app with keys of its own handles them first and ends its
/// binding with this. Nothing while the box is not focused, so that the key
/// is not captured and goes to the app.
pub fn key_binding<M>(press: KeyPress, on_action: impl Fn(Action) -> M) -> Option<Binding<M>> {
    if !matches!(press.status, text_editor::Status::Focused { .. }) {
        return None;
    }
    let ctrl = press.modifiers.command() && !press.modifiers.alt() && !press.modifiers.shift();
    let own = history_key(&press.key, press.physical_key, press.modifiers).or(match press.key {
        Key::Named(Named::Backspace) if ctrl => Some(Action::DeleteWordBefore),
        Key::Named(Named::Delete) if ctrl => Some(Action::DeleteWordAfter),
        _ => None,
    });
    match own {
        Some(action) => Some(Binding::Custom(on_action(action))),
        None => Binding::from_key_press(press),
    }
}

/// `content`, a text editor or what wraps one, with Shift+click: a left
/// press over it with Shift held publishes `extend`, the box's
/// [`Action::Extend`], before the press itself, so that the click extends
/// the selection rather than placing the caret
pub fn shift_click<'a, M: Clone + 'a>(
    content: impl Into<Element<'a, M>>,
    extend: M,
) -> Element<'a, M> {
    Element::new(ShiftClick {
        content: content.into(),
        extend,
    })
}

struct ShiftClick<'a, M> {
    content: Element<'a, M>,
    extend: M,
}

/// The modifiers held, as the window last said
#[derive(Default)]
struct Held(Modifiers);

impl<M: Clone> Widget<M, cosmic::Theme, cosmic::Renderer> for ShiftClick<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Held>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Held::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &cosmic::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &cosmic::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let held = tree.state.downcast_mut::<Held>();
        match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => held.0 = *modifiers,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if held.0.shift() && cursor.is_over(layout.bounds()) =>
            {
                shell.publish(self.extend.clone());
            }
            _ => {}
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

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
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
        renderer: &cosmic::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &cosmic::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, cosmic::Theme, cosmic::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }

    fn drag_destinations(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        renderer: &cosmic::Renderer,
        dnd_rectangles: &mut cosmic::iced::advanced::clipboard::DndDestinationRectangles,
    ) {
        self.content.as_widget().drag_destinations(
            &tree.children[0],
            layout,
            renderer,
            dnd_rectangles,
        );
    }
}
