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
//! **Unfocused, the selection is kept and drawn dimmed**, as GTK draws a
//! window's selection when it is not the focus: what the menu's Cut or Copy
//! acts on is always seen. [`editor`] gives the box its colours and [`wrap`]
//! draws it so; see [`kept_selection_colours`] for the look.
//!
//! **One-line boxes** are [`crate::short_text_box`]: the same history, steps
//! and keys over a value the app keeps.

use std::cell::Cell;
use std::collections::VecDeque;
use std::ops::Deref;

use cosmic::Element;
use cosmic::iced::advanced::layout::{self, Layout};
use cosmic::iced::advanced::renderer;
use cosmic::iced::advanced::widget::operation::Focusable;
use cosmic::iced::advanced::widget::{Operation, Tree, Widget, tree};
use cosmic::iced::advanced::{Clipboard, Shell, mouse, overlay};
use cosmic::iced::keyboard::{self, Key, Modifiers, key::Named, key::Physical};
use cosmic::iced::{Color, Event, Length, Point, Rectangle, Size, Vector};
use cosmic::widget::menu::key_bind::{KeyBind, Modifier};
use cosmic::widget::text_editor::{self, Binding, Cursor, Edit, KeyPress, Motion, Position};

/// The steps Ctrl+Z can go back
pub const STEPS: usize = 300;

/// The most text a box's history keeps, in bytes, the oldest steps going
/// first: a long draft pasted again and again stays in bounds
pub const BYTES: usize = 8 << 20;

pub use crate::kept::kept_selection_colours;

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
pub(crate) trait Kept {
    fn bytes(&self) -> usize;
}

/// The steps a box can go back and forward through: each the state before
/// it (in `undo`) or after it (in `redo`), and the app's own part of it, if
/// any. `S` is a box's whole state, so that going back is putting it back: a
/// text editor's text and caret, a one-line box's value and caret. At most
/// [`STEPS`] steps and [`BYTES`] bytes. One history for both kinds of box,
/// so that Ctrl+Z goes back by the same steps in each.
#[derive(Debug)]
pub(crate) struct History<S, P> {
    pub(crate) undo: VecDeque<(S, Option<P>)>,
    pub(crate) redo: Vec<(S, Option<P>)>,
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
    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The app's part of the step Undo would take back, if it has one
    pub(crate) fn undo_part(&self) -> Option<&P> {
        self.undo.back().and_then(|(_, part)| part.as_ref())
    }

    /// The app's part of the step Redo would bring back, if it has one
    pub(crate) fn redo_part(&self) -> Option<&P> {
        self.redo.last().and_then(|(_, part)| part.as_ref())
    }

    /// A step taken from `before`, with the app's `part` of it: what Redo
    /// would have brought back is gone
    pub(crate) fn record(&mut self, before: S, part: Option<P>) {
        self.drop_redo();
        self.bytes += before.bytes();
        self.undo.push_back((before, part));
        while self.undo.len() > STEPS || (self.bytes > BYTES && self.undo.len() > 1) {
            let (oldest, _) = self.undo.pop_front().expect("a step");
            self.bytes -= oldest.bytes();
        }
    }

    /// The state before the last step and its part, `now` kept for Redo
    pub(crate) fn undo(&mut self, now: S) -> Option<(S, Option<P>)> {
        let (before, part) = self.undo.pop_back()?;
        self.bytes = self.bytes + now.bytes() - before.bytes();
        self.redo.push((now, part.clone()));
        Some((before, part))
    }

    /// The state after the step last undone and its part, `now` kept for
    /// Undo
    pub(crate) fn redo(&mut self, now: S) -> Option<(S, Option<P>)> {
        let (after, part) = self.redo.pop()?;
        self.bytes = self.bytes + now.bytes() - after.bytes();
        self.undo.push_back((now, part.clone()));
        Some((after, part))
    }

    /// What Redo would have brought back is gone, the steps before kept
    pub(crate) fn drop_redo(&mut self) {
        self.bytes -= self.redo.drain(..).map(|(s, _)| s.bytes()).sum::<usize>();
    }

    /// The history ends at the nearest step with a part either way: it,
    /// and all beyond it, gone
    pub(crate) fn cut_at_parts(&mut self) {
        if let Some(at) = self.undo.iter().rposition(|(_, part)| part.is_some()) {
            self.bytes -= self
                .undo
                .drain(..=at)
                .map(|(s, _)| s.bytes())
                .sum::<usize>();
        }
        if let Some(at) = self.redo.iter().rposition(|(_, part)| part.is_some()) {
            self.bytes -= self
                .redo
                .drain(..=at)
                .map(|(s, _)| s.bytes())
                .sum::<usize>();
        }
    }
}

/// The run of typing or deleting going on, which the next edit of the same
/// kind joins as part of the same step
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Run {
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

    /// Whether an edit of the kind `next` (`None`: a paste, a cut, a word
    /// deleted, anything else) is part of the step `going` on: typing over
    /// a selection starts a step, which the typing after it joins
    pub(crate) fn joins(going: Option<Run>, next: Option<Run>, selected: bool) -> bool {
        !selected
            && match (going, next) {
                (Some(going), Some(next)) => going.joined_by(next),
                _ => false,
            }
    }
}

/// A box with an undo history, as a menu's Undo and Redo show it
/// ([`crate::TextMenuHistory::of`]): a [`TextBox`] or a
/// [`ShortTextBox`](crate::ShortTextBox)
pub trait Undoable {
    /// The box's action, as its widget publishes it
    type Action;
    /// The action that undoes, and the one that redoes
    const UNDO: Self::Action;
    const REDO: Self::Action;
    fn can_undo(&self) -> bool;
    fn can_redo(&self) -> bool;
}

impl<P: Clone> Undoable for TextBox<P> {
    type Action = Action;
    const UNDO: Action = Action::Undo;
    const REDO: Action = Action::Redo;

    fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
}

/// A text editor's whole state, to go back to: its text, its caret and
/// selection, and how it selects (a word or line selected by a double or
/// triple click is put back as one, not as the point clicked)
#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    text: String,
    cursor: Cursor,
    selecting: Selecting,
}

/// How a selection was made: iced's editor keeps a double or triple
/// click's as the point clicked, which selects the words or lines round it
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Selecting {
    Plain,
    Words,
    Lines,
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
/// undone, end the history there ([`forget_parts`](Self::forget_parts)).
/// Without parts, `P` is `()`: a `let` then names the type, `let draft:
/// TextBox = TextBox::new()`.
pub struct TextBox<P = ()> {
    content: Content,
    history: History<Snapshot, P>,
    /// The run of typing or deleting going on, if any
    run: Option<Run>,
    /// Where the selection began before the click just done, for its
    /// [`Action::Extend`], and whether iced took the click for a double or
    /// triple one; only ever the last action's
    clicked_from: Option<(Position, bool)>,
    /// Where the last single click was, in the editor: a double or triple
    /// click is one near it, which iced does not place
    last_click: Option<Point>,
}

impl<P> Default for TextBox<P> {
    fn default() -> Self {
        Self {
            content: Content::default(),
            history: History::default(),
            run: None,
            clicked_from: None,
            last_click: None,
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
        self.history.can_undo()
    }

    /// Whether Redo would change anything
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
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
        self.history.undo_part()
    }

    /// The app's part of the step Redo would bring back, if it has one
    pub fn redo_part(&self) -> Option<&P> {
        self.history.redo_part()
    }

    /// The steps with an app's part can no longer be undone or redone (the
    /// app's side of them changed): the history ends at the nearest of them
    /// either way, the steps of the text alone between kept. Going back
    /// past one would bring back text its part took elsewhere
    pub fn forget_parts(&mut self) {
        self.history.cut_at_parts();
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
                if let Some((from, again)) = clicked_from {
                    // A double or triple click left the caret where it was:
                    // it goes where the click was, as a single click's
                    if again && let Some(at) = self.last_click {
                        self.content.perform(text_editor::Action::Click(at));
                    }
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
            // iced's editor clears its selection when it loses the focus (a
            // menu opened, another widget clicked): a text box keeps it, as
            // in GTK and the browsers, for the menu's Cut and Copy and for
            // when the focus comes back
            Action::Editor(text_editor::Action::ClearSelection) => self.run = None,
            Action::Editor(text_editor::Action::Edit(edit)) => self.edit(edit),
            // A scroll leaves the caret where it is
            Action::Editor(scroll @ text_editor::Action::Scroll { .. }) => {
                self.content.perform(scroll);
            }
            Action::Editor(action) => {
                self.run = None;
                let clicked = match action {
                    text_editor::Action::Click(at) => {
                        self.last_click = Some(at);
                        Some(false)
                    }
                    text_editor::Action::SelectWord | text_editor::Action::SelectLine => Some(true),
                    _ => None,
                };
                let from = clicked.map(|again| (self.selection_from(), again));
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
        let joins = Run::joins(self.run, run, selected);
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
        let cursor = self.content.cursor();
        let selecting = self.selecting(&text, cursor);
        Snapshot {
            text,
            cursor,
            selecting,
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        let Snapshot {
            text,
            cursor,
            selecting,
        } = snapshot;
        let (anchor, select) = match (cursor.selection, selecting) {
            (Some(anchor), Selecting::Words) => (anchor, text_editor::Action::SelectWord),
            (Some(anchor), Selecting::Lines) => (anchor, text_editor::Action::SelectLine),
            _ => return self.set(&text, Caret::At(cursor)),
        };
        // Selected as it was: from the point clicked, the caret where it was
        self.set(
            &text,
            Caret::At(Cursor {
                position: anchor,
                selection: None,
            }),
        );
        self.content.perform(select);
        self.content.move_to(Cursor {
            position: cursor.position,
            selection: None,
        });
    }

    /// How the selection of `cursor` in `text` was made: by its ends, or a
    /// double or triple click's
    fn selecting(&self, text: &str, cursor: Cursor) -> Selecting {
        let (Some(anchor), Some(selected)) = (cursor.selection, self.content.selection()) else {
            return Selecting::Plain;
        };
        let starts = line_starts(text);
        let offset = |at: Position| starts[at.line.min(starts.len() - 1)] + at.column;
        let (caret, anchor_at) = (offset(cursor.position), offset(anchor));
        if text.get(caret.min(anchor_at)..caret.max(anchor_at)) == Some(selected.as_str()) {
            return Selecting::Plain;
        }
        let (first, last) = (
            cursor.position.line.min(anchor.line),
            cursor.position.line.max(anchor.line),
        );
        let lines: Vec<&str> = text.split('\n').collect();
        let whole = lines.get(first..=last).map(|lines| lines.join("\n"));
        if whole.as_deref() == Some(selected.trim_end_matches('\n')) {
            Selecting::Lines
        } else {
            Selecting::Words
        }
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

/// The keys a text box adds to iced's (household rule 21c), and what each
/// does: Ctrl+Z undoes; Ctrl+Shift+Z, and Ctrl+Y as Cerea and Marginalium
/// also have it, redo; Ctrl+Backspace and Ctrl+Delete delete a word. The one
/// list: [`key_binding`] and [`history_key`] go by it, and an app shows it
/// (its keyboard shortcuts, its menus). Any keyboard layout matches, by the
/// key's place ([`KeyBind::matches`])
pub fn standard_keys() -> Vec<(KeyBind, Action)> {
    let ctrl = |key: Key| KeyBind {
        modifiers: vec![Modifier::Ctrl],
        key,
    };
    let letter = |c: &str| Key::Character(c.into());
    vec![
        (ctrl(letter("z")), Action::Undo),
        (
            KeyBind {
                modifiers: vec![Modifier::Ctrl, Modifier::Shift],
                key: letter("z"),
            },
            Action::Redo,
        ),
        (ctrl(letter("y")), Action::Redo),
        (ctrl(Key::Named(Named::Backspace)), Action::DeleteWordBefore),
        (ctrl(Key::Named(Named::Delete)), Action::DeleteWordAfter),
    ]
}

/// What a standard key does ([`standard_keys`]), if it is one
pub(crate) fn standard_key(key: &Key, physical: Physical, modifiers: Modifiers) -> Option<Action> {
    standard_keys()
        .into_iter()
        .find(|(bind, _)| bind.matches(modifiers, key, Some(&physical)))
        .map(|(_, action)| action)
}

/// What a key does to a box's history: Undo or Redo, by [`standard_keys`].
/// For a box that is given keys some other way than as a focused widget
/// (the terminal's prompt, typed into while the conversation has the keys)
pub fn history_key(key: &Key, physical: Physical, modifiers: Modifiers) -> Option<Action> {
    standard_key(key, physical, modifiers)
        .filter(|action| matches!(action, Action::Undo | Action::Redo))
}

/// The standard keys of a text box (see the module's description), for
/// `text_editor(..).key_binding(..)`: iced's, with [`standard_keys`]
/// added. `on_action` is the box's message, as given to `on_action`. An app
/// with keys of its own handles them first and ends its binding with this.
/// Nothing while the box is not focused, so that the key is not captured
/// and goes to the app.
pub fn key_binding<M>(press: KeyPress, on_action: impl Fn(Action) -> M) -> Option<Binding<M>> {
    if !matches!(press.status, text_editor::Status::Focused { .. }) {
        return None;
    }
    match standard_key(&press.key, press.physical_key, press.modifiers) {
        Some(action) => Some(Binding::Custom(on_action(action))),
        None => Binding::from_key_press(press),
    }
}

/// A text box's editor: libcosmic's `text_editor` over `text_box`, its
/// actions to `on_action`, the standard keys ([`key_binding`]), and the
/// colours of a selection kept while it is unfocused
/// ([`kept_selection_colours`]); a focused box's selection stays
/// libcosmic's, the solid accent and its on colour. An app with keys of its
/// own sets its own `.key_binding(..)`, ending with [`key_binding`]. Put
/// [`wrap`] round it (and round its text menu, if it has one).
pub fn editor<'a, P: Clone, M: Clone + 'static>(
    text_box: &'a TextBox<P>,
    on_action: impl Fn(Action) -> M + Clone + 'a,
) -> cosmic::widget::text_editor::TextEditor<'a, M> {
    let keys = on_action.clone();
    cosmic::widget::text_editor::text_editor(&text_box.content)
        .on_action(move |action| on_action(action.into()))
        .key_binding(move |press| key_binding(press, &keys))
        .selection_fill(selection_fill)
        .selected_text_color(selected_text)
}

/// The fill behind selected text: libcosmic's, or the kept look
fn selection_fill(theme: &cosmic::Theme, style: &text_editor::Style) -> Color {
    match crate::kept::drawing() {
        Some(_) => kept_selection_colours(theme, style.value, beneath(theme, style)).0,
        None => style.selection,
    }
}

/// The colour of selected text: libcosmic's (its editor's own choice, the
/// accent's on colour), or the kept look
fn selected_text(theme: &cosmic::Theme, style: &text_editor::Style) -> Color {
    match crate::kept::drawing() {
        Some(_) => kept_selection_colours(theme, style.value, beneath(theme, style)).1,
        None => theme.cosmic().accent.on.into(),
    }
}

/// What a kept selection lies over: the editor's background
fn beneath(theme: &cosmic::Theme, style: &text_editor::Style) -> Color {
    crate::kept::flat(style.background, theme.cosmic().bg_color().into())
}

/// `content`, a text box's [`editor`] or what wraps it (its text menu), as
/// a text box: Shift+click extends the selection (as [`shift_click`]), and
/// the selection, kept while the box is unfocused, is drawn dimmed. Its
/// actions go to `on_action`, as the editor's
pub fn wrap<'a, P: Clone, M: Clone + 'a>(
    content: impl Into<Element<'a, M>>,
    text_box: &TextBox<P>,
    on_action: impl Fn(Action) -> M,
) -> Element<'a, M> {
    Element::new(Frame {
        content: content.into(),
        extend: on_action(Action::Extend),
        selected: text_box.content.cursor().selection.is_some(),
    })
}

/// `content`, a text editor or what wraps one, with Shift+click: a left
/// press with Shift held is the editor's own click, then `extend`, the
/// box's [`Action::Extend`], so that the click extends the selection. Only
/// a press the editor acted on is extended (one that can be changed, or
/// selected in). [`wrap`] does this and draws a kept selection
pub fn shift_click<'a, M: Clone + 'a>(
    content: impl Into<Element<'a, M>>,
    extend: M,
) -> Element<'a, M> {
    Element::new(Frame {
        content: content.into(),
        extend,
        selected: false,
    })
}

/// What [`wrap`] and [`shift_click`] put round a text editor
struct Frame<'a, M> {
    content: Element<'a, M>,
    extend: M,
    /// Whether the box has a selection, which is kept while unfocused
    selected: bool,
}

/// iced's editor state, as libcosmic's text editor holds it
type EditorState = text_editor::State<cosmic::iced::advanced::text::highlighter::PlainText>;

#[derive(Default)]
struct FrameState {
    /// The modifiers held, as the window last said
    held: Modifiers,
    /// The content's tree with its editor focused, while the box is
    /// unfocused with a kept selection: what it is drawn from
    kept: Option<Tree>,
}

impl<M> Frame<'_, M> {
    /// The tree a kept selection is drawn from, made or let go as the box
    /// is now
    fn keep(&self, tree: &mut Tree) {
        let unfocused = crate::kept::find::<EditorState>(&tree.children[0])
            .is_some_and(|editor| !editor.is_focused());
        let state = tree.state.downcast_mut::<FrameState>();
        if !(self.selected && unfocused) {
            state.kept = None;
            return;
        }
        if state
            .kept
            .as_ref()
            .is_some_and(|kept| kept.tag == tree.children[0].tag)
        {
            return;
        }
        let mut kept = Tree::new(self.content.as_widget());
        if let Some(editor) = crate::kept::find_mut::<EditorState>(&mut kept) {
            Focusable::focus(editor);
        }
        state.kept = Some(kept);
    }
}

impl<M: Clone> Widget<M, cosmic::Theme, cosmic::Renderer> for Frame<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<FrameState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(FrameState::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
        if let Some(kept) = &mut tree.state.downcast_mut::<FrameState>().kept {
            kept.diff(self.content.as_widget_mut());
            if let Some(editor) = crate::kept::find_mut::<EditorState>(kept) {
                Focusable::focus(editor);
            }
        }
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &cosmic::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let node = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        self.keep(tree);
        node
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
        let held = &mut tree.state.downcast_mut::<FrameState>().held;
        let extends = match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                *held = *modifiers;
                false
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                held.shift() && cursor.is_over(layout.bounds())
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
            self.keep(tree);
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
        self.keep(tree);
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
        let draw = |tree: &Tree, renderer: &mut cosmic::Renderer| {
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        };
        match &tree.state.downcast_ref::<FrameState>().kept {
            Some(kept) => crate::kept::drawn(cursor.is_over(layout.bounds()), || {
                draw(kept, renderer);
            }),
            None => draw(&tree.children[0], renderer),
        }
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
