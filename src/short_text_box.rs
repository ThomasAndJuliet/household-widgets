// SPDX-License-Identifier: GPL-3.0-only

//! The short text box: a one-line field that does what a text box does.
//! Juliet, 8 Nov 2026: "Yes we should have a "short text box" widget and
//! much of the code can be shared between the text box and short text box
//! but there are also obviously different."
//!
//! libcosmic's `text_input` is the field; this adds what it lacks, as
//! [`crate::text_box`] does for the editor, without changing libcosmic.
//!
//! **Shared with the text box** (one copy, in [`crate::history`] and
//! [`crate::text_box`]): the history and its bounds, the rules that make
//! steps (a run of typing ended by moving the caret, by a space after a
//! word, by changing between typing and deleting; a paste, a cut, a word
//! deleted and every change the app makes a step each), the app's own part
//! of a step ([`Undoable::replace_with`], [`Turn`], household rule 19b),
//! everything [`Undoable`] offers, the keys
//! ([`standard_keys`](crate::text_box::standard_keys): Ctrl+Z,
//! Ctrl+Shift+Z and Ctrl+Y, the box's only while it is focused), the text
//! menu's Undo · Redo ([`crate::TextMenuHistory::of`]), and a selection kept
//! while unfocused, drawn dimmed
//! ([`kept_selection_colours`](crate::text_box::kept_selection_colours)).
//!
//! **Different**, as one-line fields are:
//!
//! - The app keeps the value, as with `text_input`: a [`ShortTextBox`],
//!   which derefs to its `str`. The caret is the widget's, so the widget
//!   reports where it was and is with each edit ([`Edit`]), and puts back the
//!   caret and selection an Undo restores: the box asks for it and the
//!   widget does it when it is next built, so the app needs no task. Where
//!   the app puts the caret is a [`Caret`] of this module, in graphemes.
//! - Enter submits ([`Field::on_submit`]); there are no new lines.
//! - Ctrl+Backspace and Ctrl+Delete are libcosmic's own, by words (to the
//!   start or end in a password); Shift+click, which it lacks, is added.
//! - A password is a [`PasswordBox`], which only [`secure`] shows, and
//!   `secure` shows nothing else: it keeps no history, as in GTK (nothing
//!   typed into it is kept for Undo, Ctrl+Z does nothing), and nothing of
//!   it is written out by `Debug`.
//! - Fields are normal text boxes in the household's look ([`Look`], rule
//!   25a); [`Field::behind`] says the field's window is behind others.
//!
//! **The kinds** are libcosmic's own constructors, one function each, all
//! giving one widget, [`Field`]: [`text`] (`text_input`), [`search`]
//! (`search_input`), [`secure`] (`secure_input`), [`inline`]
//! (`inline_input`) and [`editable`] (`editable_input`). The look of each
//! stays libcosmic's, and the rest of what libcosmic's input offers is
//! passed on to it by `Field`'s builder methods.
//!
//! ```ignore
//! // the app's state: search: ShortTextBox, and Message::Search(Action)
//! text_menu(short_text_box::search(&self.search, "", Message::Search), &words, style)
//!     .history(TextMenuHistory::of(&self.search, Message::Search))
//! // in update: Message::Search(action) => { self.search.perform(action); .. }
//! ```
//!
//! A value the app sets is one step Ctrl+Z takes back
//! ([`Undoable::replace`]; the same value again is nothing); a value that is
//! where the field starts (a page number shown as the pages move) is a new
//! box, [`ShortTextBox::starting_with`], with nothing to undo.

use std::borrow::Cow;
use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use zeroize::Zeroize;

use cosmic::iced::advanced::layout::{self, Layout};
use cosmic::iced::advanced::renderer;
use cosmic::iced::advanced::widget::{Id, Operation, Tree, Widget, tree};
use cosmic::iced::advanced::{Clipboard, Shell, input_method, mouse, overlay};
use cosmic::iced::keyboard::{self, Key, Modifiers, key::Named};
use cosmic::iced::{Event, Font, Length, Padding, Pixels, Point, Rectangle, Size, Vector, touch};
use cosmic::widget::text_context_menu::{TextAction, TextMenuContext, TextMenuTarget};
use cosmic::widget::text_input::{self, Appearance, StyleSheet, TextInput, cursor, value::Value};
use cosmic::{Element, Renderer, Theme};

use crate::history::{History, Run, Turn, Undoable};
use crate::kept;
use crate::look::{Look, State};
use crate::text_box::{self, history_key};

/// Where the app puts a field's caret when it sets the value, in graphemes
/// of the value (what a reader takes for one character, as libcosmic's
/// input counts)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caret {
    /// At the start
    Start,
    /// At the end, to go on typing
    End,
    /// Here, with the selection from `anchor` (kept inside the value)
    At { anchor: usize, caret: usize },
}

/// Where the caret is, and the other end of the selection (the caret
/// itself without one), in graphemes of the value
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) anchor: usize,
    pub(crate) caret: usize,
}

impl Span {
    fn at(caret: usize) -> Self {
        Self {
            anchor: caret,
            caret,
        }
    }

    fn selects(self) -> bool {
        self.anchor != self.caret
    }
}

/// What made an edit, as far as steps need: typing (a key, or an input
/// method's text), a character deleted, or anything else (a paste, a cut,
/// a word deleted)
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum By {
    /// Typing; whether it ended with a space
    Typed {
        space: bool,
    },
    Deleted,
    Other,
}

/// An edit the field made, as it reports it: the new value, where the
/// caret was before and is after, and the key that made it
#[derive(Clone, PartialEq)]
pub struct Edit {
    pub(crate) value: String,
    pub(crate) before: Span,
    pub(crate) after: Span,
    pub(crate) by: By,
    /// A password's or a sensitive box's: its value is not written out
    pub(crate) hidden: bool,
}

impl std::fmt::Debug for Edit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value: &dyn std::fmt::Debug = match self.hidden {
            true => &"(hidden)",
            false => &self.value,
        };
        f.debug_struct("Edit")
            .field("value", value)
            .field("before", &self.before)
            .field("after", &self.after)
            .field("by", &self.by)
            .finish()
    }
}

/// What a [`ShortTextBox`] is asked to do, by its [`Field`] or its menu
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// The field changed its value
    Edit(Edit),
    /// Back one step (Ctrl+Z)
    Undo,
    /// Forward one step (Ctrl+Shift+Z, Ctrl+Y)
    Redo,
}

/// A field's whole state, to go back to
#[doc(hidden)]
pub struct Line {
    text: String,
    span: Span,
}

impl Line {
    /// Its text zeroized: a sensitive box's history lets go of it
    fn wipe(&mut self) {
        self.text.zeroize();
    }
}

/// Each request to put the caret somewhere is newer than the last, so that
/// a field puts it there once
static REQUESTS: AtomicU64 = AtomicU64::new(1);

/// A short text box's value and undo history, which the app keeps; see the
/// module's description. It derefs to the value. Changes go through
/// [`perform`](Self::perform), [`Undoable::replace`] and
/// [`Undoable::replace_with`], or a new box
/// ([`starting_with`](Self::starting_with)). `P` is the app's own part of
/// a step, as [`crate::TextBox`]'s.
///
/// A sensitive box ([`sensitive`](Self::sensitive)) is a box for values
/// that are not secrets but must not linger (a card's name and expiry): its
/// Undo and Redo are as ever, but nothing of it stays in memory it lets go
/// of. The value it replaces, and the copy an edit brought once taken in,
/// are zeroized; its history zeroizes each step it lets go of and all it
/// holds when dropped; dropping it zeroizes its value; `Debug` writes none
/// of it, nor do its field's edits
pub struct ShortTextBox<P = ()> {
    value: String,
    /// Where the caret was after the last change the box knows of
    pub(crate) span: Span,
    history: History<Line, P>,
    /// Where the field is to put its caret, and the request's number
    pub(crate) put: Option<(u64, Span)>,
}

impl<P> Default for ShortTextBox<P> {
    fn default() -> Self {
        Self::with_history(String::new(), Span::default(), History::default())
    }
}

impl<P> Drop for ShortTextBox<P> {
    fn drop(&mut self) {
        if self.history.is_sensitive() {
            self.value.zeroize();
        }
    }
}

impl<P> Deref for ShortTextBox<P> {
    type Target = str;

    fn deref(&self) -> &str {
        &self.value
    }
}

impl<P> std::fmt::Debug for ShortTextBox<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value: &dyn std::fmt::Debug = match self.history.is_sensitive() {
            true => &"(hidden)",
            false => &self.value,
        };
        f.debug_struct("ShortTextBox")
            .field("value", value)
            .field("history", &self.history.len())
            .finish()
    }
}

impl<P> ShortTextBox<P> {
    fn with_history(value: String, span: Span, history: History<Line, P>) -> Self {
        Self {
            value,
            span,
            history,
            put: None,
        }
    }
}

impl<P: Clone> ShortTextBox<P> {
    /// An empty box, with nothing to undo
    pub fn new() -> Self {
        Self::default()
    }

    /// A box starting with `text`, with nothing to undo: where the field
    /// starts, not a change to it
    pub fn starting_with(text: &str) -> Self {
        Self::with_history(
            text.to_string(),
            Span::at(Value::new(text).len()),
            History::default(),
        )
    }

    /// An empty sensitive box (see the type's description), with nothing to
    /// undo
    pub fn sensitive() -> Self {
        Self::with_history(
            String::new(),
            Span::default(),
            History::sensitive(Line::wipe),
        )
    }

    /// Whether it is a sensitive box
    pub fn is_sensitive(&self) -> bool {
        self.history.is_sensitive()
    }

    /// The value
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Does `action`, keeping the history. A step undone or redone that has
    /// the app's part hands it back, for the app to do its side
    pub fn perform(&mut self, action: Action) -> Option<Turn<P>> {
        self.perform_part(action, None)
    }

    /// [`perform`](Self::perform), a step the field's edit starts carrying
    /// `part`, the app's own side of it (rule 19b): a search field's typing
    /// moves the reader off their match, which Undo brings back. An edit
    /// joining the step going on leaves that step's part as it was
    pub fn perform_with(&mut self, action: Action, part: P) -> Option<Turn<P>> {
        self.perform_part(action, Some(part))
    }

    fn perform_part(&mut self, action: Action, part: Option<P>) -> Option<Turn<P>> {
        match action {
            Action::Edit(edit) => {
                self.edit(edit, part);
                None
            }
            Action::Undo => self.turn(true),
            Action::Redo => self.turn(false),
        }
    }

    /// The field's edit: part of the run going on, or a step of its own
    fn edit(&mut self, edit: Edit, part: Option<P>) {
        let Edit {
            value,
            before,
            after,
            by,
            ..
        } = edit;
        if value == self.value {
            // A key that changed nothing (Backspace at the start) is no run
            self.span = after;
            self.history.end_run();
            self.let_go(value);
            return;
        }
        // The caret moved since the last edit: the run going on has ended
        if before != self.span {
            self.history.end_run();
        }
        let selected = before.selects();
        let run = match by {
            By::Typed { space } => Some(Run::Typing { space }),
            // Deleting a selection is a step of its own
            By::Deleted if !selected => Some(Run::Deleting),
            By::Deleted | By::Other => None,
        };
        if !self.history.joins(run, selected) {
            let text = std::mem::take(&mut self.value);
            let bytes = text.len();
            self.history
                .record(Line { text, span: before }, bytes, part);
        }
        self.take_value(value);
        self.span = after;
    }

    /// `value` in place of the value: what it replaces let go of
    fn take_value(&mut self, value: String) {
        let old = std::mem::replace(&mut self.value, value);
        self.let_go(old);
    }

    /// `text` let go of: zeroized, if the box is sensitive
    fn let_go(&self, mut text: String) {
        if self.history.is_sensitive() {
            text.zeroize();
        }
    }

    /// The caret is to go to `span`: the field puts it there when next built
    fn put(&mut self, span: Span) {
        self.span = span;
        self.put = Some(request(span));
    }
}

impl<P: Clone> Undoable for ShortTextBox<P> {
    type Action = Action;
    type Part = P;
    type Caret = Caret;
    type State = Line;
    const UNDO: Action = Action::Undo;
    const REDO: Action = Action::Redo;

    fn history(&self) -> &History<Line, P> {
        &self.history
    }

    fn history_mut(&mut self) -> &mut History<Line, P> {
        &mut self.history
    }

    fn state(&self) -> (Line, usize) {
        let line = Line {
            text: self.value.clone(),
            span: self.span,
        };
        (line, self.value.len())
    }

    fn restore(&mut self, mut line: Line) {
        self.take_value(std::mem::take(&mut line.text));
        self.put(line.span);
    }

    fn set(&mut self, text: &str, caret: Caret) {
        self.take_value(text.to_string());
        self.put(span_for(text, caret));
        self.history.end_run();
    }

    fn holds(&self, text: &str) -> bool {
        self.value == text
    }

    fn unchanged(&mut self, _: Caret) {}
}

/// Where `caret` is in `text`, kept inside it
fn span_for(text: &str, caret: Caret) -> Span {
    let length = Value::new(text).len();
    match caret {
        Caret::Start => Span::at(0),
        Caret::End => Span::at(length),
        Caret::At { anchor, caret } => Span {
            anchor: anchor.min(length),
            caret: caret.min(length),
        },
    }
}

/// A request to put a field's caret at `span`, newer than any before
fn request(span: Span) -> (u64, Span) {
    (REQUESTS.fetch_add(1, Ordering::Relaxed), span)
}

/// A password's value, which the app keeps, with no history: nothing typed
/// into it is kept for Undo, as in GTK, and nothing of it is written out by
/// `Debug`. Only [`secure`] shows it, and `secure` shows nothing else, so
/// a password cannot be shown in a plain field, nor a plain box keep a
/// password's history.
///
/// Nothing of a password stays behind in memory the box lets go of: each
/// change zeroizes the value it replaces, in its own buffer, and the copy
/// the change brought once it is taken in; dropping the box zeroizes its
/// value; [`take`](Self::take) hands the value over and leaves the box
/// empty
#[derive(Default)]
pub struct PasswordBox {
    value: String,
    put: Option<(u64, Span)>,
}

impl PasswordBox {
    /// An empty password box
    pub fn new() -> Self {
        Self::default()
    }

    /// The value
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Does `action`: the field's edit sets the value; Undo and Redo do
    /// nothing, there being no history
    pub fn perform(&mut self, action: Action) {
        match action {
            Action::Edit(edit) => replace_zeroized(&mut self.value, edit.value),
            Action::Undo | Action::Redo => {}
        }
    }

    /// The value set by the app (a password cleared once used), the caret
    /// at `caret`
    pub fn set(&mut self, text: &str, caret: Caret) {
        self.value.zeroize();
        self.value.push_str(text);
        self.put = Some(request(span_for(text, caret)));
    }

    /// The value, handed over (to what checks it): the box is empty again,
    /// keeping nothing of it, and the field's caret goes to its start
    pub fn take(&mut self) -> String {
        self.put = Some(request(Span::at(0)));
        std::mem::take(&mut self.value)
    }

    /// The whole of the value's buffer, its spare capacity too, for the
    /// tests of what a change leaves in it; only once a change has
    /// zeroized the buffer is all of it written
    #[cfg(test)]
    pub(crate) fn buffer(&self) -> &[u8] {
        // SAFETY: the pointer and capacity are the String's own allocation;
        // the tests call this only after a change zeroized the whole
        // capacity (`replace_zeroized`, `set`), so every byte of it is written
        unsafe { std::slice::from_raw_parts(self.value.as_ptr(), self.value.capacity()) }
    }
}

impl Drop for PasswordBox {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

/// `slot` holds `new` in place of what it held: what it held is zeroized
/// first, the whole of its buffer, so that growing it leaves zeros behind;
/// then `new` is copied in and its own buffer zeroized
fn replace_zeroized(slot: &mut String, mut new: String) {
    slot.zeroize();
    slot.push_str(&new);
    new.zeroize();
}

impl Deref for PasswordBox {
    type Target = str;

    fn deref(&self) -> &str {
        &self.value
    }
}

impl std::fmt::Debug for PasswordBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PasswordBox")
            .field("value", &"(hidden)")
            .finish()
    }
}

/// What libcosmic's input says, inside a [`Field`]: a value typed or pasted,
/// or one of the app's own messages (Enter, a button, the focus)
#[derive(Clone, Debug)]
enum Change<M> {
    Input(String),
    Pasted(String),
    App(M),
}

/// A field's style: libcosmic's for its kind (`base`), in the household's
/// look ([`Look`]), with the look of a kept selection when that is drawn:
/// libcosmic draws a selection only for a focused input, so a kept one is
/// drawn as focused, and the border is the unfocused one. An editable
/// field (`editable`) keeps libcosmic's label look until it is edited
fn field_style(
    base: Rc<cosmic::theme::TextInput>,
    look: Look,
    editable: bool,
    invalid: bool,
) -> cosmic::theme::TextInput {
    let style = move || base.clone();
    let (active, error, hovered, focused, disabled) = (style(), style(), style(), style(), style());
    // (a field at fault: libcosmic's error look in every state, one border)
    let looks = move |theme: &Theme, base: &cosmic::theme::TextInput, state| match invalid {
        true => theme.error(base),
        false => field_look(theme, base, look, state, editable),
    };
    cosmic::theme::TextInput::Custom {
        active: Box::new(move |theme| looks(theme, &active, State::Rest)),
        // An input in its error state is drawn so, even as focused: a kept
        // selection in it takes the kept colours there
        error: Box::new(move |theme| match kept::drawing() {
            Some(_) => kept_colours(theme, theme.error(&error)),
            None => theme.error(&error),
        }),
        hovered: Box::new(move |theme| looks(theme, &hovered, State::Hovered)),
        focused: Box::new(move |theme| match kept::drawing() {
            Some(over) => {
                let state = if over { State::Hovered } else { State::Rest };
                kept_colours(theme, looks(theme, &focused, state))
            }
            None => looks(theme, &focused, State::Focused),
        }),
        disabled: Box::new(move |theme| theme.disabled(&disabled)),
    }
}

/// libcosmic's look of `base` in `state`, on the page's colour and with the
/// border of `look`'s kind
fn field_look(
    theme: &Theme,
    base: &cosmic::theme::TextInput,
    look: Look,
    state: State,
    editable: bool,
) -> Appearance {
    let mut appearance = match state {
        State::Rest => theme.active(base),
        State::Hovered => theme.hovered(base),
        State::Focused => theme.focused(base),
    };
    if editable && state != State::Focused {
        return appearance;
    }
    let border = look.border(theme, state);
    appearance.background = crate::look::background(theme);
    appearance.border_offset = None;
    appearance.border_width = border.width;
    appearance.border_color = border.color;
    appearance.border_radius = border.radius;
    appearance
}

/// `appearance` with a kept selection's colours
fn kept_colours(theme: &Theme, mut appearance: Appearance) -> Appearance {
    let container = theme.current_container();
    let text = appearance.text_color.unwrap_or_else(|| container.on.into());
    let beneath = kept::flat(appearance.background, container.base.into());
    (appearance.selected_fill, appearance.selected_text_color) =
        kept::kept_selection_colours(theme, text, beneath);
    appearance
}

/// A short text box shown: libcosmic's text input over a
/// [`ShortTextBox`], publishing its [`Action`]s; made by [`text`],
/// [`search`], [`secure`], [`inline`] or [`editable`]. Wrap it in
/// [`crate::text_menu`] for its menu, with [`crate::TextMenuHistory::of`]
/// the box for Undo · Redo.
pub struct Field<'a, M> {
    input: TextInput<'a, Change<M>>,
    messages: Messages<'a, M>,
    /// Where the box asks for the caret to go, and the request's number
    put: Option<(u64, Span)>,
    /// libcosmic's style for the kind of input, which the look is drawn on
    base: Rc<cosmic::theme::TextInput>,
    /// An editable label's input, looking like text until it is edited
    editable: bool,
    /// What the input's overlays publish, made when they are asked for
    overlay_map: Option<Box<dyn Fn(Change<M>) -> M + 'a>>,
    /// The characters it takes as they are typed, if not all
    accepts: Option<fn(char) -> bool>,
    /// Its kind of box and whether its window is behind others
    look: Look,
    /// Something is wrong with what it holds: libcosmic's error look
    invalid: bool,
}

/// The app's messages for what a field's input says
struct Messages<'a, M> {
    on_action: Rc<dyn Fn(Action) -> M + 'a>,
    on_paste: Option<Rc<dyn Fn(String) -> M + 'a>>,
    /// A password's: it has no history to undo or redo
    hidden: bool,
    /// A password's or a sensitive box's: what is typed is not written out
    redacted: bool,
}

impl<M> Clone for Messages<'_, M> {
    fn clone(&self) -> Self {
        Self {
            on_action: self.on_action.clone(),
            on_paste: self.on_paste.clone(),
            hidden: self.hidden,
            redacted: self.redacted,
        }
    }
}

impl<M> Messages<'_, M> {
    /// The app's message for what the input said, the caret `before` and
    /// `after` it, made `by` a key; and the app's own message for a paste,
    /// if it asked for one, into `also`
    fn message(
        &self,
        change: Change<M>,
        before: Span,
        after: cursor::Cursor,
        by: By,
        also: &mut Vec<M>,
    ) -> M {
        // Where the caret is in the new value (libcosmic's input does not
        // always note the new value itself first: an input method's text)
        let edit = |value: String, by| {
            let after = span_in(after, &Value::new(&value));
            (self.on_action)(Action::Edit(Edit {
                value,
                before,
                after,
                by,
                hidden: self.redacted,
            }))
        };
        match change {
            Change::Input(value) => edit(value, by),
            // libcosmic's input sends what an input method commits as a
            // paste: it is typing, and no paste of the app's
            Change::Pasted(value) if matches!(by, By::Typed { .. }) => edit(value, by),
            Change::Pasted(value) => {
                if let Some(on_paste) = &self.on_paste {
                    also.push(on_paste(value.clone()));
                }
                edit(value, By::Other)
            }
            Change::App(message) => message,
        }
    }
}

/// A short text box, as libcosmic's `text_input`
pub fn text<'a, P: Clone, M: Clone + 'static>(
    text_box: &'a ShortTextBox<P>,
    placeholder: impl Into<Cow<'a, str>>,
    on_action: impl Fn(Action) -> M + 'a,
) -> Field<'a, M> {
    let input = text_input::text_input(placeholder, text_box.value.as_str());
    Field::new(
        input,
        cosmic::theme::TextInput::Default,
        false,
        (text_box.put, false, text_box.is_sensitive()),
        on_action,
    )
}

/// A search box, as libcosmic's `search_input`: its clear button is shown
/// with [`Field::on_clear`]
pub fn search<'a, P: Clone, M: Clone + 'static>(
    text_box: &'a ShortTextBox<P>,
    placeholder: impl Into<Cow<'a, str>>,
    on_action: impl Fn(Action) -> M + 'a,
) -> Field<'a, M> {
    let input = text_input::search_input(placeholder, text_box.value.as_str());
    Field::new(
        input,
        cosmic::theme::TextInput::Search,
        false,
        (text_box.put, false, text_box.is_sensitive()),
        on_action,
    )
}

/// A box in the text it stands in, as libcosmic's `inline_input` (a name
/// renamed where it is shown)
pub fn inline<'a, P: Clone, M: Clone + 'static>(
    text_box: &'a ShortTextBox<P>,
    placeholder: impl Into<Cow<'a, str>>,
    on_action: impl Fn(Action) -> M + 'a,
) -> Field<'a, M> {
    let input = text_input::inline_input(placeholder, text_box.value.as_str());
    Field::new(
        input,
        cosmic::theme::TextInput::Inline,
        false,
        (text_box.put, false, text_box.is_sensitive()),
        on_action,
    )
}

/// Text shown as a label until its edit button is pressed, as libcosmic's
/// `editable_input`: `editing` says whether it is being edited, and
/// `on_toggle_edit` is told when that changes
pub fn editable<'a, P: Clone, M: Clone + 'static>(
    text_box: &'a ShortTextBox<P>,
    placeholder: impl Into<Cow<'a, str>>,
    on_action: impl Fn(Action) -> M + 'a,
    editing: bool,
    on_toggle_edit: impl Fn(bool) -> M + 'a,
) -> Field<'a, M> {
    let input = text_input::editable_input(
        placeholder,
        text_box.value.as_str(),
        editing,
        move |editing| Change::App(on_toggle_edit(editing)),
    );
    Field::new(
        input,
        cosmic::theme::TextInput::EditableText,
        true,
        (text_box.put, false, text_box.is_sensitive()),
        on_action,
    )
}

/// A password box, as libcosmic's `secure_input`, over a [`PasswordBox`]
/// (which keeps no history): hidden while `hidden`, with its show-or-hide
/// button publishing `on_toggle`, if given
pub fn secure<'a, M: Clone + 'static>(
    text_box: &'a PasswordBox,
    placeholder: impl Into<Cow<'a, str>>,
    on_action: impl Fn(Action) -> M + 'a,
    hidden: bool,
    on_toggle: Option<M>,
) -> Field<'a, M> {
    let input = text_input::secure_input(
        placeholder,
        text_box.value.as_str(),
        on_toggle.map(Change::App),
        hidden,
    );
    Field::new(
        input,
        cosmic::theme::TextInput::Default,
        false,
        (text_box.put, true, true),
        on_action,
    )
}

/// A password's box shown as a plain one-line field, drawn as [`text`]
/// draws one: a secret the person must see as it is typed, which is kept as
/// a password is (no history, nothing of it written out, zeroized as it
/// goes): a password generated for them, a page's prompt answered. To
/// mask it, or show it on request, use [`secure`]
pub fn shown<'a, M: Clone + 'static>(
    text_box: &'a PasswordBox,
    placeholder: impl Into<Cow<'a, str>>,
    on_action: impl Fn(Action) -> M + 'a,
) -> Field<'a, M> {
    let input = text_input::text_input(placeholder, text_box.value.as_str());
    Field::new(
        input,
        cosmic::theme::TextInput::Default,
        false,
        (text_box.put, true, true),
        on_action,
    )
}

impl<'a, M: Clone + 'static> Field<'a, M> {
    /// A field over libcosmic's `input` of the kind drawn in `base`; `put`,
    /// where its box asks for the caret, whether it is a password's, and
    /// whether what is typed is not to be written out (a password's, a
    /// sensitive box's)
    fn new(
        input: TextInput<'a, Change<M>>,
        base: cosmic::theme::TextInput,
        editable: bool,
        (put, hidden, redacted): (Option<(u64, Span)>, bool, bool),
        on_action: impl Fn(Action) -> M + 'a,
    ) -> Self {
        let base = Rc::new(base);
        Self {
            input: input
                .on_input(Change::Input)
                .on_paste(Change::Pasted)
                .style(field_style(base.clone(), Look::normal(), editable, false)),
            messages: Messages {
                on_action: Rc::new(on_action),
                on_paste: None,
                hidden,
                redacted,
            },
            put,
            base,
            editable,
            overlay_map: None,
            accepts: None,
            look: Look::normal(),
            invalid: false,
        }
    }

    /// Something is wrong with what it holds (a form's field at fault):
    /// drawn in libcosmic's error look, its one border the error colour
    /// and its fill libcosmic's, in every state, in place of the household
    /// border (rule 25a: "error borders keep libcosmic's"). The app says
    /// what is wrong where it says it, as libcosmic's own `error` would
    /// write it under the field
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self.restyle()
    }

    /// The input's style, as the field's look and state are now
    fn restyle(self) -> Self {
        let style = field_style(self.base.clone(), self.look, self.editable, self.invalid);
        self.with(|input| input.style(style))
    }

    /// Only the characters `accepts` takes are typed: a key or an input
    /// method's text with another is not given to the input at all (a host
    /// takes no space, a number no letter). The input edits a copy of its
    /// value as each key comes, before the app answers, so a filter the app
    /// applies afterwards is too late for the keys already come; this one is
    /// before them, whatever their timing. What is pasted is the app's to
    /// filter, as it is given whole
    pub fn accepts(mut self, accepts: fn(char) -> bool) -> Self {
        self.accepts = Some(accepts);
        self
    }

    /// Whether the field's window is behind others: its focused border is
    /// then the rest colour (rule 25a, a normal text box)
    pub fn behind(mut self, behind: bool) -> Self {
        self.look = Look::normal().behind(behind);
        self.restyle()
    }

    /// libcosmic's input, changed by `change`
    fn with(
        mut self,
        change: impl FnOnce(TextInput<'a, Change<M>>) -> TextInput<'a, Change<M>>,
    ) -> Self {
        self.input = change(self.input);
        self
    }

    /// Enter publishes `message`
    pub fn on_submit(self, message: M) -> Self {
        self.with(|input| input.on_submit(move |_| Change::App(message.clone())))
    }

    /// A search box's clear button, shown while it has text, publishes
    /// `message`: the app clears the box ([`Undoable::replace`] or
    /// `replace_with`, a step Undo takes back)
    pub fn on_clear(self, message: M) -> Self {
        self.with(|input| input.on_clear(Change::App(message)))
    }

    /// `message` when a click gives the field the keyboard
    pub fn on_focus(self, message: M) -> Self {
        self.with(|input| input.on_focus(Change::App(message)))
    }

    /// `message` when Tab or Esc takes the keyboard from the field
    pub fn on_unfocus(self, message: M) -> Self {
        self.with(|input| input.on_unfocus(Change::App(message)))
    }

    /// Tab publishes `message` and stays in the field
    pub fn on_tab(self, message: M) -> Self {
        self.with(|input| input.on_tab(Change::App(message)))
    }

    /// A paste also publishes `on_paste` of what was pasted, after the
    /// box's own edit
    pub fn on_paste(mut self, on_paste: impl Fn(String) -> M + 'a) -> Self {
        self.messages.on_paste = Some(Rc::new(on_paste));
        self
    }

    pub fn id(self, id: Id) -> Self {
        self.with(|input| input.id(id))
    }

    pub fn width(self, width: impl Into<Length>) -> Self {
        self.with(|input| input.width(width))
    }

    pub fn padding(self, padding: impl Into<Padding>) -> Self {
        self.with(|input| input.padding(padding))
    }

    pub fn size(self, size: impl Into<Pixels>) -> Self {
        self.with(|input| input.size(size))
    }

    pub fn font(self, font: Font) -> Self {
        self.with(|input| input.font(font))
    }

    /// A label above the field
    pub fn label(self, label: impl Into<Cow<'a, str>>) -> Self {
        self.with(|input| input.label(label))
    }

    /// A line of help below the field
    pub fn helper_text(self, helper_text: impl Into<Cow<'a, str>>) -> Self {
        self.with(|input| input.helper_text(helper_text))
    }

    /// The field in its error look, saying `error`
    pub fn error(self, error: impl Into<Cow<'a, str>>) -> Self {
        self.with(|input| input.error(error))
    }

    /// Everything selected when the field takes the keyboard
    pub fn select_on_focus(self, select_on_focus: bool) -> Self {
        self.with(|input| input.select_on_focus(select_on_focus))
    }

    pub fn leading_icon(self, icon: impl Into<Element<'a, M>>) -> Self {
        let icon = icon.into().map(Change::App);
        self.with(|input| input.leading_icon(icon))
    }

    pub fn trailing_icon(self, icon: impl Into<Element<'a, M>>) -> Self {
        let icon = icon.into().map(Change::App);
        self.with(|input| input.trailing_icon(icon))
    }

    /// `run` on the input, with what it says turned into the app's
    /// messages, each with where the caret was before and after
    fn forward(
        &mut self,
        input: &mut Tree,
        by: By,
        shell: &mut Shell<'_, M>,
        run: impl FnOnce(&mut TextInput<'a, Change<M>>, &mut Tree, &mut Shell<'_, Change<M>>),
    ) {
        let before = span_of(input);
        let mut changes = Vec::new();
        let mut own = Shell::new(&mut changes);
        run(&mut self.input, input, &mut own);
        let after = state(input).cursor();
        let also = RefCell::new(Vec::new());
        shell.merge(own, |change| {
            self.messages
                .message(change, before, after, by, &mut also.borrow_mut())
        });
        for message in also.into_inner() {
            shell.publish(message);
        }
    }
}

/// libcosmic's input state in a field's tree
fn state(input: &Tree) -> &text_input::State {
    input.state.downcast_ref::<text_input::State>()
}

fn state_mut(input: &mut Tree) -> &mut text_input::State {
    input.state.downcast_mut::<text_input::State>()
}

/// Where the input's caret and selection are
fn span_of(input: &Tree) -> Span {
    let state = state(input);
    span_in(state.cursor(), &state.tracked_value)
}

/// Where `cursor` is in `value`
fn span_in(cursor: cursor::Cursor, value: &Value) -> Span {
    match cursor.state(value) {
        cursor::State::Index(at) => Span::at(at),
        cursor::State::Selection { start, end } => Span {
            anchor: start,
            caret: end,
        },
    }
}

/// The text an event types: a key's with no Ctrl, Alt or Super held, or
/// what an input method commits
fn typed_text(event: &Event) -> Option<&str> {
    match event {
        Event::InputMethod(input_method::Event::Commit(text)) => Some(text),
        Event::Keyboard(keyboard::Event::KeyPressed {
            text: Some(text),
            modifiers,
            ..
        }) if !(modifiers.control() || modifiers.alt() || modifiers.logo()) => Some(text),
        _ => None,
    }
}

/// What an event does to steps: typing (a key, or what an input method
/// commits), a character deleted, or something else (with Ctrl, a word
/// deleted, a paste, a cut)
fn by_key(event: &Event) -> By {
    let typed = |text: &str| By::Typed {
        space: text.chars().last().is_some_and(char::is_whitespace),
    };
    match event {
        Event::InputMethod(input_method::Event::Commit(text)) => typed(text),
        Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            text,
            modifiers,
            ..
        }) => {
            if modifiers.control() || modifiers.alt() || modifiers.logo() {
                return By::Other;
            }
            match key {
                Key::Named(Named::Backspace) => By::Deleted,
                // Shift+Delete is Cut
                Key::Named(Named::Delete) if !modifiers.shift() => By::Deleted,
                _ => text
                    .as_deref()
                    .filter(|text| text.chars().any(|c| !c.is_control()))
                    .map_or(By::Other, typed),
            }
        }
        _ => By::Other,
    }
}

/// A field's own state, beside the input's (its child)
#[derive(Default)]
struct FieldState {
    /// The modifiers held, as the window last said
    held: Modifiers,
    /// The last request to put the caret that was carried out
    done: u64,
    /// The input's state as it was focused, just before a press that may
    /// take the focus away while keeping its selection
    focused: Option<text_input::State>,
    /// The field's tree as it was focused, while it is unfocused with a kept
    /// selection: what it is drawn from
    kept: Option<Tree>,
}

impl<M: Clone + 'static> Field<'_, M> {
    fn as_input(&self) -> &dyn Widget<Change<M>, Theme, Renderer> {
        &self.input
    }

    /// The tree a kept selection is drawn from, made or let go as the input
    /// is now. libcosmic's input keeps its selection unfocused only when a
    /// press elsewhere took the focus (Esc and Tab clear it), and its state
    /// cannot be focused again without taking the focus from another input,
    /// so the state is the one copied just before that press: a field that
    /// never had the focus has none
    fn keep(&self, tree: &mut Tree) {
        let input = state(&tree.children[0]);
        let own = tree.state.downcast_mut::<FieldState>();
        let selected = input.cursor().selection(&input.tracked_value).is_some();
        if input.is_focused() || !selected {
            own.focused = None;
            own.kept = None;
            return;
        }
        if let Some(focused) = own.focused.take() {
            let mut kept = Tree::new(self.as_input());
            kept.state = tree::State::new(focused);
            own.kept = Some(kept);
        }
        // The value changed since (the app set it), or a password was shown
        // or hidden: the selection drawn would be of the old one
        if own.kept.as_ref().is_some_and(|kept| {
            state(kept).tracked_value != input.tracked_value
                || state(kept).cursor() != input.cursor()
                || state(kept).is_secure != input.is_secure
        }) {
            own.kept = None;
        }
    }
}

impl<'a, M: Clone + 'static> Widget<M, Theme, Renderer> for Field<'a, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<FieldState>()
    }

    fn state(&self) -> tree::State {
        // A field shown anew does not carry out a request made before it
        tree::State::new(FieldState {
            done: self.put.map_or(0, |(request, _)| request),
            ..FieldState::default()
        })
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.as_input())]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.children[0].diff(&mut self.input as &mut dyn Widget<Change<M>, Theme, Renderer>);
        let own = tree.state.downcast_mut::<FieldState>();
        if let Some(kept) = &mut own.kept {
            kept.children = Tree::new(self.as_input()).children;
        }
        // The caret and selection put where the box asks (an Undo)
        if let Some((request, span)) = self.put
            && request > own.done
        {
            own.done = request;
            let input = state_mut(&mut tree.children[0]);
            if span.selects() {
                input.select_range(span.anchor, span.caret);
            } else {
                input.move_cursor_to(span.caret);
            }
        }
    }

    fn size(&self) -> Size<Length> {
        self.as_input().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.as_input().size_hint()
    }

    // The input carries the field's identity
    fn id(&self) -> Option<Id> {
        None
    }

    fn set_id(&mut self, id: Id) {
        Widget::<Change<M>, Theme, Renderer>::set_id(&mut self.input, id);
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let node = self.input.layout(&mut tree.children[0], renderer, limits);
        self.keep(tree);
        node
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.input
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
        let own = tree.state.downcast_mut::<FieldState>();
        if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = event {
            own.held = *modifiers;
        }
        let held = own.held;
        let input = &mut tree.children[0];
        let focused = state(input).is_focused();
        // Undo and redo, the field's only while it is focused: otherwise the
        // key goes on to the app (its own Ctrl+Z). A password's field takes
        // them and does nothing
        if focused
            && let Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                physical_key,
                modifiers,
                ..
            }) = event
            && let Some(back) = match history_key(key, *physical_key, *modifiers) {
                Some(text_box::Action::Undo) => Some(true),
                Some(text_box::Action::Redo) => Some(false),
                Some(
                    text_box::Action::Editor(_)
                    | text_box::Action::DeleteWordBefore
                    | text_box::Action::DeleteWordAfter
                    | text_box::Action::Extend,
                )
                | None => None,
            }
        {
            if !self.messages.hidden {
                let action = match back {
                    true => Action::Undo,
                    false => Action::Redo,
                };
                shell.publish((self.messages.on_action)(action));
            }
            shell.capture_event();
            return;
        }
        // A character the field does not take, typed: not given to the input
        if focused
            && let Some(accepts) = self.accepts
            && let Some(text) = typed_text(event)
            && text.chars().any(|c| !c.is_control() && !accepts(c))
        {
            shell.capture_event();
            return;
        }
        let pressed = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(_))
                | Event::Touch(touch::Event::FingerPressed { .. })
        );
        // A press may take the focus and keep the selection: the state as
        // it is focused, for drawing the selection kept
        if focused && pressed && span_of(input).selects() {
            tree.state.downcast_mut::<FieldState>().focused = Some(state(input).clone());
        }
        let input = &mut tree.children[0];
        // Shift+click: from where the selection began to the click. The
        // selection goes first, so that the click is a plain one
        let extend = (focused
            && held.shift()
            && matches!(
                event,
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            )
            && cursor.is_over(layout.bounds()))
        .then(|| {
            let anchor = span_of(input).anchor;
            state_mut(input).move_cursor_to(anchor);
            anchor
        });
        self.forward(input, by_key(event), shell, |field, input, own| {
            field.update(
                input, event, layout, cursor, renderer, clipboard, own, viewport,
            );
        });
        if let Some(anchor) = extend {
            let to = span_of(input);
            // A click quick after another selects a word: the selection
            // runs to the word's far end, as GTK extends by words then
            let caret = match to.anchor.min(to.caret) {
                low if anchor <= low => to.anchor.max(to.caret),
                low => low,
            };
            state_mut(input).select_range(anchor, caret);
        }
        self.keep(tree);
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
        match &tree.state.downcast_ref::<FieldState>().kept {
            Some(kept) => kept::drawn(cursor.is_over(layout.bounds()), || {
                self.as_input()
                    .draw(kept, renderer, theme, style, layout, cursor, viewport);
            }),
            None => self.as_input().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            ),
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.input
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
        // What the input's overlays publish, the caret as it is now
        let input = &tree.children[0];
        let (at, cursor) = (span_of(input), state(input).cursor());
        let messages = self.messages.clone();
        let map = self.overlay_map.insert(Box::new(move |change| {
            messages.message(change, at, cursor, By::Other, &mut Vec::new())
        }));
        let map: &'b dyn Fn(Change<M>) -> M = &**map;
        self.input
            .overlay(
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
            )
            .map(|overlay| overlay.map(map))
    }

    fn drag_destinations(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        destinations: &mut cosmic::iced::advanced::clipboard::DndDestinationRectangles,
    ) {
        self.input
            .drag_destinations(&tree.children[0], layout, renderer, destinations);
    }

    #[cfg(feature = "a11y")]
    fn a11y_nodes(
        &self,
        layout: Layout<'_>,
        tree: &Tree,
        cursor: mouse::Cursor,
    ) -> iced_accessibility::A11yTree {
        self.input.a11y_nodes(layout, &tree.children[0], cursor)
    }
}

impl<M: Clone + 'static> TextMenuTarget<M> for Field<'_, M> {
    fn use_external_text_menu(&mut self) {
        self.input.use_external_text_menu();
    }

    fn take_text_menu_request(&self, tree: &mut Tree) -> Option<Point> {
        self.input.take_text_menu_request(&mut tree.children[0])
    }

    fn text_menu_context(&self, tree: &Tree) -> TextMenuContext {
        self.input.text_menu_context(&tree.children[0])
    }

    fn execute_text_menu_action(
        &mut self,
        tree: &mut Tree,
        action: TextAction,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
    ) {
        self.forward(
            &mut tree.children[0],
            By::Other,
            shell,
            |field, input, own| {
                field.execute_text_menu_action(input, action, clipboard, own);
            },
        );
    }
}

impl<'a, M: Clone + 'static> From<Field<'a, M>> for Element<'a, M> {
    fn from(field: Field<'a, M>) -> Self {
        Element::new(field)
    }
}
