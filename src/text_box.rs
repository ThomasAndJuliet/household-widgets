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
//!   [`TextBox::replace`] (or [`TextBox::amend`]), so every change can be
//!   undone; a new box ([`TextBox::starting_with`]) has nothing to undo.
//!   Where the caret goes when the app sets the text is always said:
//!   [`Caret`].
//! - [`Action`]: what the box is asked to do: iced's own actions, and
//!   Undo, Redo, deleting a word, and Shift+click.
//! - [`key_binding`]: the standard keys (household rule 21c). An app with
//!   keys of its own handles those first and ends its binding with this.
//!   [`history_key`] is its Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y alone, for keys
//!   that reach a box some other way.
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
//! [`STEPS`] steps, and at most [`BYTES`] of text. Undo puts back the caret
//! and selection as they were, a word or line selected by a double or
//! triple click too.
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

use std::cell::Cell;
use std::collections::VecDeque;
use std::ops::Deref;

use cosmic::Element;
use cosmic::iced::advanced::layout::{self, Layout};
use cosmic::iced::advanced::renderer;
use cosmic::iced::advanced::widget::{Operation, Tree, Widget, tree};
use cosmic::iced::advanced::{Clipboard, Shell, mouse, overlay};
use cosmic::iced::keyboard::{self, Key, Modifiers, key::Named, key::Physical};
use cosmic::iced::{Event, Length, Rectangle, Size, Vector};
use cosmic::widget::text_editor::{self, Binding, Cursor, Edit, KeyPress, Motion, Position};

/// The steps Ctrl+Z can go back
pub const STEPS: usize = 300;

/// The most text a box's history keeps, in bytes, the oldest steps going
/// first: a long draft pasted again and again stays in bounds
pub const BYTES: usize = 8 << 20;

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
    /// Shift was held for the click just done ([`shift_click`] sends it
    /// straight after the click's own action): the selection runs from
    /// where it began before the click to the caret, rather than the click
    /// placing the caret or, quick after another click, selecting a word
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

/// Where the caret goes when the app sets the text
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Caret {
    /// At the start of the text
    Start,
    /// At its end, to go on typing
    End,
    /// Here, with this selection (kept inside the text)
    At(Cursor),
}

/// A state a history keeps, and what it costs to keep
trait Kept {
    fn bytes(&self) -> usize;
}

/// The steps a box can go back and forward through: each the state before
/// it (in `undo`) or after it (in `redo`), and the app's own part of it, if
/// any. `S` is a box's whole state, so that going back is putting it back: a
/// text editor's text and caret, a one-line box's value. At most [`STEPS`]
/// steps and [`BYTES`] bytes.
#[derive(Debug)]
struct History<S, P> {
    undo: VecDeque<(S, Option<P>)>,
    redo: Vec<(S, Option<P>)>,
    /// What `undo` and `redo` hold, in bytes
    bytes: usize,
}

impl<S, P> Default for History<S, P> {
    fn default() -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            bytes: 0,
        }
    }
}

impl<S: Kept, P: Clone> History<S, P> {
    /// A step taken from `before`, with the app's `part` of it: what Redo
    /// would have brought back is gone
    fn record(&mut self, before: S, part: Option<P>) {
        self.drop_redo();
        self.bytes += before.bytes();
        self.undo.push_back((before, part));
        while self.undo.len() > STEPS || (self.bytes > BYTES && self.undo.len() > 1) {
            let (oldest, _) = self.undo.pop_front().expect("a step");
            self.bytes -= oldest.bytes();
        }
    }

    /// The state before the last step and its part, `now` kept for Redo
    fn undo(&mut self, now: S) -> Option<(S, Option<P>)> {
        let (before, part) = self.undo.pop_back()?;
        self.bytes = self.bytes + now.bytes() - before.bytes();
        self.redo.push((now, part.clone()));
        Some((before, part))
    }

    /// The state after the step last undone and its part, `now` kept for
    /// Undo
    fn redo(&mut self, now: S) -> Option<(S, Option<P>)> {
        let (after, part) = self.redo.pop()?;
        self.bytes = self.bytes + now.bytes() - after.bytes();
        self.undo.push_back((now, part.clone()));
        Some((after, part))
    }

    /// What Redo would have brought back is gone, the steps before kept
    fn drop_redo(&mut self) {
        self.bytes -= self.redo.drain(..).map(|(s, _)| s.bytes()).sum::<usize>();
    }

    /// What Undo would have taken back is gone, the steps Redo has kept
    fn drop_undo(&mut self) {
        self.bytes -= self.undo.drain(..).map(|(s, _)| s.bytes()).sum::<usize>();
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

/// A text editor's whole state, to go back to: its text, and its caret and
/// selection with both ends as they are (a word or line selected by a
/// double or triple click too)
#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    text: String,
    cursor: Cursor,
}

impl Kept for Snapshot {
    fn bytes(&self) -> usize {
        self.text.len()
    }
}

/// A text box's text and caret, with its undo history; see the module's
/// description. It derefs to iced's [`Content`], for the editor widget and
/// for reading; changes go through [`perform`](Self::perform),
/// [`replace`](Self::replace), [`replace_with`](Self::replace_with) and
/// [`amend`](Self::amend), or a new box
/// ([`starting_with`](Self::starting_with)).
///
/// `P` is the app's own part of a step, for a step that is more than the
/// text ([`replace_with`](Self::replace_with)): a prompt stashed is also a
/// card added. Undo and Redo hand it back ([`Turn`]) for the app to undo or
/// redo its part. Before, the app can look at it
/// ([`undo_part`](Self::undo_part)) and, if its part can no longer be
/// undone, end the history there ([`forget_undo`](Self::forget_undo)).
/// Without parts, `P` is `()`: a `let` then names the type, `let draft:
/// TextBox = TextBox::new()`.
pub struct TextBox<P = ()> {
    content: Content,
    history: History<Snapshot, P>,
    /// The run of typing or deleting going on, if any
    run: Option<Run>,
    /// Where the selection began before the click just done, for its
    /// [`Action::Extend`]; only ever the last action's
    clicked_from: Option<Position>,
}

impl<P> Default for TextBox<P> {
    fn default() -> Self {
        Self {
            content: Content::default(),
            history: History::default(),
            run: None,
            clicked_from: None,
        }
    }
}

/// A step with the app's part undone or redone: the part, for the app to
/// undo or redo its own side of it
#[derive(Clone, Debug, PartialEq)]
pub enum Turn<P> {
    Undone(P),
    Redone(P),
}

impl<P> Deref for TextBox<P> {
    type Target = Content;

    fn deref(&self) -> &Content {
        &self.content
    }
}

impl<P> std::fmt::Debug for TextBox<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextBox")
            .field("text", &self.content.text())
            .field("cursor", &self.content.cursor())
            .field("undo", &self.history.undo.len())
            .field("redo", &self.history.redo.len())
            .finish()
    }
}

impl<P: Clone> TextBox<P> {
    /// An empty box, with nothing to undo
    pub fn new() -> Self {
        Self::default()
    }

    /// A box starting with `text`, the caret at `caret`, with nothing to
    /// undo: a draft opened, or a new one
    pub fn starting_with(text: &str, caret: Caret) -> Self {
        let mut text_box = Self::default();
        text_box.set(text, caret);
        text_box
    }

    /// Whether Undo would change anything
    pub fn can_undo(&self) -> bool {
        !self.history.undo.is_empty()
    }

    /// Whether Redo would change anything
    pub fn can_redo(&self) -> bool {
        !self.history.redo.is_empty()
    }

    /// The text replaced by the app, the caret at `caret`: one step, which
    /// Undo takes back
    pub fn replace(&mut self, text: &str, caret: Caret) {
        let before = self.snapshot();
        self.set(text, caret);
        if self.content.text() != before.text {
            self.history.record(before, None);
        }
    }

    /// The text replaced by the app as one step with `part`, the app's own
    /// part of it, which Undo and Redo hand back: always a step, the text
    /// changed or not
    pub fn replace_with(&mut self, text: &str, caret: Caret, part: P) {
        let before = self.snapshot();
        self.set(text, caret);
        self.history.record(before, Some(part));
    }

    /// The app's part of the step Undo would take back, if it has one
    pub fn undo_part(&self) -> Option<&P> {
        self.history.undo.back().and_then(|(_, part)| part.as_ref())
    }

    /// The app's part of the step Redo would bring back, if it has one
    pub fn redo_part(&self) -> Option<&P> {
        self.history.redo.last().and_then(|(_, part)| part.as_ref())
    }

    /// Nothing more to undo: a step whose app's part can no longer be
    /// undone ends the history there (going back past it would bring the
    /// text back without the rest)
    pub fn forget_undo(&mut self) {
        self.history.drop_undo();
    }

    /// Nothing more to redo, for a step whose app's part can no longer be
    /// redone
    pub fn forget_redo(&mut self) {
        self.history.drop_redo();
    }

    /// The text replaced by the app as part of its last step, which Undo
    /// takes back whole: a change made in steps of its own, as the saves of
    /// a message open in another editor. What Redo had is gone
    pub fn amend(&mut self, text: &str, caret: Caret) {
        self.set(text, caret);
        self.history.drop_redo();
    }

    /// Does `action`, keeping the history. A step undone or redone that has
    /// the app's part hands it back, for the app to do its side
    pub fn perform(&mut self, action: impl Into<Action>) -> Option<Turn<P>> {
        let clicked_from = self.clicked_from.take();
        match action.into() {
            Action::Undo => {
                if self.can_undo() {
                    let now = self.snapshot();
                    let (before, part) = self.history.undo(now).expect("a step to undo");
                    self.restore(before);
                    return part.map(Turn::Undone);
                }
            }
            Action::Redo => {
                if self.can_redo() {
                    let now = self.snapshot();
                    let (after, part) = self.history.redo(now).expect("a step to redo");
                    self.restore(after);
                    return part.map(Turn::Redone);
                }
            }
            Action::DeleteWordBefore => self.delete_word(Motion::WordLeft, Edit::Backspace),
            Action::DeleteWordAfter => self.delete_word(Motion::WordRight, Edit::Delete),
            // From where the selection began to where the click put the
            // caret: after a double or triple click, the caret was not
            // moved, and the word or line it selected gives way
            Action::Extend => {
                if let Some(from) = clicked_from {
                    let position = self.content.cursor().position;
                    self.content.perform(text_editor::Action::ClearSelection);
                    if from != position {
                        self.content.move_to(Cursor {
                            position,
                            selection: Some(from),
                        });
                    }
                }
            }
            Action::Editor(text_editor::Action::Edit(edit)) => self.edit(edit),
            // A scroll leaves the caret where it is
            Action::Editor(scroll @ text_editor::Action::Scroll { .. }) => {
                self.content.perform(scroll);
            }
            Action::Editor(action) => {
                self.run = None;
                let click = matches!(
                    action,
                    text_editor::Action::Click(_)
                        | text_editor::Action::SelectWord
                        | text_editor::Action::SelectLine
                );
                let from = click.then(|| self.selection_from());
                self.content.perform(action);
                self.clicked_from = from;
            }
        }
        None
    }

    /// An edit of iced's: part of the run going on, or a step of its own
    fn edit(&mut self, edit: Edit) {
        let selected = self.content.cursor().selection.is_some();
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
        let selected = self.content.cursor().selection.is_some();
        self.change(|content| {
            if !selected {
                content.perform(text_editor::Action::Select(motion));
            }
            content.perform(text_editor::Action::Edit(edit));
        });
    }

    /// `change` made as a step of its own, if it changes the text; whether
    /// it did. The text is read whole once, for the step; whether it
    /// changed is told by the caret, the number of lines and the caret's
    /// line, which every change of the text moves or changes
    fn change(&mut self, change: impl FnOnce(&mut Content)) -> bool {
        let before = self.snapshot();
        let mark = self.mark();
        change(&mut self.content);
        let changed = self.mark() != mark;
        if changed {
            self.history.record(before, None);
        }
        changed
    }

    /// What a change of the text changes: see [`change`](Self::change)
    fn mark(&self) -> (usize, Cursor, Option<String>) {
        let cursor = self.content.cursor();
        let line = self
            .content
            .line(cursor.position.line)
            .map(|line| line.text.into_owned());
        (self.content.line_count(), cursor, line)
    }

    /// The text set, the caret at `caret`; no step
    fn set(&mut self, text: &str, caret: Caret) {
        self.content = Content::with_text(text);
        match caret {
            Caret::Start => {}
            Caret::End => self
                .content
                .perform(text_editor::Action::Move(Motion::DocumentEnd)),
            Caret::At(cursor) => {
                let cursor = Cursor {
                    position: self.within(cursor.position),
                    selection: cursor.selection.map(|at| self.within(at)),
                };
                self.content.move_to(cursor);
            }
        }
        self.run = None;
        self.clicked_from = None;
    }

    fn snapshot(&self) -> Snapshot {
        let text = self.content.text();
        let cursor = self.selection_ends(&text);
        Snapshot { text, cursor }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.set(&snapshot.text, Caret::At(snapshot.cursor));
    }

    /// Where the selection begins, or the caret if there is none: what a
    /// Shift+click extends from
    fn selection_from(&self) -> Position {
        let cursor = self.selection_ends(&self.content.text());
        cursor.selection.unwrap_or(cursor.position)
    }

    /// The caret and the selection's other end, as they are: iced reports a
    /// word or line selection (a double or triple click) by the point
    /// clicked, so its ends are found from the text it selects, which lies
    /// round that point and the caret
    fn selection_ends(&self, text: &str) -> Cursor {
        let cursor = self.content.cursor();
        let (Some(anchor), Some(selected)) = (cursor.selection, self.content.selection()) else {
            return Cursor {
                position: cursor.position,
                selection: None,
            };
        };
        let starts = line_starts(text);
        let offset = |at: Position| starts[at.line.min(starts.len() - 1)] + at.column;
        let (caret, anchor) = (offset(cursor.position), offset(anchor));
        let (low, high) = (caret.min(anchor), caret.max(anchor));
        // A plain selection: its ends are the caret and the anchor
        if text.get(low..high) == Some(selected.as_str()) {
            return cursor;
        }
        let Some(start) = text
            .match_indices(selected.as_str())
            .map(|(start, _)| start)
            .find(|&start| start <= low && start + selected.len() >= high)
        else {
            return cursor;
        };
        let end = start + selected.len();
        // The caret at the end the selection grew towards; at its end for
        // a word or line just selected
        let (caret, anchor) = if caret >= anchor {
            (end, start)
        } else {
            (start, end)
        };
        let position = |offset: usize| {
            let line = starts.partition_point(|&start| start <= offset) - 1;
            Position {
                line,
                column: offset - starts[line],
            }
        };
        Cursor {
            position: position(caret),
            selection: Some(position(anchor)),
        }
    }

    /// `at`, moved inside the text: on a line it has, at a character
    /// boundary of that line
    fn within(&self, at: Position) -> Position {
        let line = at.line.min(self.content.line_count().saturating_sub(1));
        let text = self
            .content
            .line(line)
            .map(|line| line.text.into_owned())
            .unwrap_or_default();
        let mut column = at.column.min(text.len());
        while !text.is_char_boundary(column) {
            column -= 1;
        }
        Position { line, column }
    }
}

/// Where each line of `text` starts, in bytes
fn line_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .collect()
}

/// What a key does to a box's history, as household rule 21c has it:
/// Ctrl+Z undoes; Ctrl+Shift+Z, and Ctrl+Y as Cerea and Marginalium also
/// have it, redo. Any keyboard layout: by the key's Latin letter. For a box
/// that is given keys some other way than as a focused widget (the
/// terminal's prompt, typed into while the conversation has the keys)
pub fn history_key(key: &Key, physical: Physical, modifiers: Modifiers) -> Option<Action> {
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
/// press with Shift held is the editor's own click, then `extend`, the
/// box's [`Action::Extend`], so that the click extends the selection. Only
/// a press the editor acted on is extended (one that can be changed, or
/// selected in)
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
        let extends = match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                held.0 = *modifiers;
                false
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                held.0.shift() && cursor.is_over(layout.bounds())
            }
            _ => false,
        };
        if !extends {
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
            return;
        }
        // The editor's own click first, then Extend, if it acted on it
        let mut messages = Vec::new();
        let mut own = Shell::new(&mut messages);
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            &mut own,
            viewport,
        );
        let acted = Cell::new(false);
        shell.merge(own, |message| {
            acted.set(true);
            message
        });
        if acted.get() {
            shell.publish(self.extend.clone());
        }
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

/// The history's bounds, on states that say their own size
#[cfg(test)]
mod history_tests {
    use super::{BYTES, History, Kept, STEPS};

    struct State(usize);

    impl Kept for State {
        fn bytes(&self) -> usize {
            self.0
        }
    }

    #[test]
    fn the_history_keeps_at_most_its_steps_and_bytes() {
        let mut history: History<State, ()> = History::default();
        for _ in 0..STEPS + 5 {
            history.record(State(1), None);
        }
        assert_eq!(history.undo.len(), STEPS);
        assert_eq!(history.bytes, STEPS);
        // Big states: the oldest go first, the newest is always kept
        let mut history: History<State, ()> = History::default();
        for n in 1..=8 {
            history.record(State(BYTES / 4 + n), None);
        }
        assert_eq!(history.undo.len(), 3);
        assert_eq!(history.undo.front().map(|(s, _)| s.0), Some(BYTES / 4 + 6));
        history.record(State(2 * BYTES), None);
        assert_eq!(
            history.undo.len(),
            1,
            "one step bigger than the bound is kept alone"
        );
        // Undo and Redo move bytes between the two, and a new step drops Redo
        let mut history: History<State, ()> = History::default();
        history.record(State(10), None);
        let (before, _) = history.undo(State(20)).expect("a step");
        assert_eq!((before.0, history.bytes), (10, 20));
        history.record(State(5), None);
        assert_eq!(history.bytes, 5);
        assert!(history.redo.is_empty());
    }
}
