// SPDX-License-Identifier: GPL-3.0-only

//! The short text box: a one-line field that does what a text box does.
//! Juliet, 8 Nov 2026: "Yes we should have a "short text box" widget and
//! much of the code can be shared between the text box and short text box
//! but there are also obviously different."
//!
//! libcosmic's `text_input` is the field; this adds what it lacks, as
//! [`crate::text_box`] does for the editor, without changing libcosmic.
//!
//! **Shared with the text box** (one copy, in [`crate::text_box`]): the
//! history and its bounds ([`STEPS`](crate::text_box::STEPS),
//! [`BYTES`](crate::text_box::BYTES)), the rules that make steps (a run of
//! typing ended by moving the caret, by a space after a word, by changing
//! between typing and deleting; a paste, a cut, a word deleted and every
//! change the app makes a step each), the app's own part of a step
//! ([`ShortTextBox::replace_with`], [`Turn`], household rule 19b), where the
//! caret goes when the app sets the text ([`Caret`]), the keys
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
//!   widget does it when it is next built, so the app needs no task.
//! - Enter submits ([`Field::on_submit`]); there are no new lines.
//! - Ctrl+Backspace and Ctrl+Delete are libcosmic's own, by words (to the
//!   start or end in a password); Shift+click, which it lacks, is added.
//! - A password ([`ShortTextBox::password`], [`secure`]) keeps no history,
//!   as in GTK: nothing typed into it is kept for Undo, Ctrl+Z does
//!   nothing, and its menu has no Undo · Redo to offer.
//!
//! **The kinds** are libcosmic's own constructors, one function each, all
//! giving one widget, [`Field`]: [`text`] (`text_input`), [`search`]
//! (`search_input`, with its clear button by [`Field::on_clear`]) and
//! [`secure`] (`secure_input`, with its show-or-hide button). The look of
//! each stays libcosmic's.
//!
//! ```ignore
//! // the app's state: search: ShortTextBox, and Message::Search(Action)
//! text_menu(short_text_box::search(&self.search, "", Message::Search), &words, style)
//!     .history(TextMenuHistory::of(&self.search, Message::Search))
//! // in update: Message::Search(action) => { self.search.perform(action); .. }
//! ```
//!
//! A value the app sets is one step Ctrl+Z takes back
//! ([`ShortTextBox::replace`]); a value that is where the field starts (a
//! page number shown as the pages move) is a new box,
//! [`ShortTextBox::starting_with`], with nothing to undo.

use std::borrow::Cow;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use cosmic::iced::advanced::layout::{self, Layout};
use cosmic::iced::advanced::renderer;
use cosmic::iced::advanced::widget::{Id, Operation, Tree, Widget, tree};
use cosmic::iced::advanced::{Clipboard, Shell, mouse, overlay};
use cosmic::iced::keyboard::{self, Key, Modifiers, key::Named};
use cosmic::iced::{Event, Length, Padding, Pixels, Point, Rectangle, Size, Vector, touch};
use cosmic::widget::text_context_menu::{TextAction, TextMenuContext, TextMenuTarget};
use cosmic::widget::text_input::{self, Appearance, StyleSheet, TextInput, cursor, value::Value};
use cosmic::{Element, Renderer, Theme};

use crate::kept;
use crate::text_box::{self, Caret, History, Kept, Run, Turn, Undoable, history_key};

/// Where the caret is, and the other end of the selection (the caret
/// itself without one), in graphemes of the value, as libcosmic's input
/// counts
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

/// What made an edit, as far as steps need: a character typed, a
/// character deleted, or anything else (a paste, a cut, a word deleted)
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum By {
    Typed(char),
    Deleted,
    Other,
}

/// An edit the field made, as it reports it: the new value, where the
/// caret was before and is after, and the key that made it
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub(crate) value: String,
    pub(crate) before: Span,
    pub(crate) after: Span,
    pub(crate) by: By,
    /// Whether it may be kept for Undo: not in a password
    pub(crate) remember: bool,
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
#[derive(Debug)]
struct Line {
    text: String,
    span: Span,
}

impl Kept for Line {
    fn bytes(&self) -> usize {
        self.text.len()
    }
}

/// Each request to put the caret somewhere is newer than the last, so that
/// a field puts it there once
static REQUESTS: AtomicU64 = AtomicU64::new(1);

/// A short text box's value and undo history, which the app keeps; see the
/// module's description. It derefs to the value. Changes go through
/// [`perform`](Self::perform), [`replace`](Self::replace) and
/// [`replace_with`](Self::replace_with), or a new box
/// ([`starting_with`](Self::starting_with)). `P` is the app's own part of
/// a step, as [`crate::TextBox`]'s.
pub struct ShortTextBox<P = ()> {
    value: String,
    /// Where the caret was after the last change the box knows of
    pub(crate) span: Span,
    history: History<Line, P>,
    /// The run of typing or deleting going on, if any
    run: Option<Run>,
    /// Where the field is to put its caret, and the request's number
    pub(crate) put: Option<(u64, Span)>,
    /// A password: no history
    password: bool,
}

impl<P> Default for ShortTextBox<P> {
    fn default() -> Self {
        Self {
            value: String::new(),
            span: Span::default(),
            history: History::default(),
            run: None,
            put: None,
            password: false,
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
        let mut debug = f.debug_struct("ShortTextBox");
        if self.password {
            debug.field("value", &"(hidden)");
        } else {
            debug.field("value", &self.value);
        }
        debug
            .field("undo", &self.history.undo.len())
            .field("redo", &self.history.redo.len())
            .finish()
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
        Self {
            value: text.to_string(),
            span: Span::at(Value::new(text).len()),
            ..Self::default()
        }
    }

    /// An empty password box, which keeps no history, as in GTK. Show it
    /// with [`secure`]
    pub fn password() -> Self {
        Self {
            password: true,
            ..Self::default()
        }
    }

    /// The value
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Whether Undo would change anything
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Whether Redo would change anything
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// The value replaced by the app, the caret at `caret`: one step,
    /// which Undo takes back (a password keeps none)
    pub fn replace(&mut self, text: &str, caret: Caret) {
        if let Some(before) = self.set(text, caret)
            && before.text != self.value
        {
            self.history.record(before, None);
        }
    }

    /// The value replaced by the app as one step with `part`, the app's
    /// own part of it, which Undo and Redo hand back: always a step, the
    /// value changed or not (a password keeps none)
    pub fn replace_with(&mut self, text: &str, caret: Caret, part: P) {
        if let Some(before) = self.set(text, caret) {
            self.history.record(before, Some(part));
        }
    }

    /// The app's part of the step Undo would take back, if it has one
    pub fn undo_part(&self) -> Option<&P> {
        self.history.undo_part()
    }

    /// The app's part of the step Redo would bring back, if it has one
    pub fn redo_part(&self) -> Option<&P> {
        self.history.redo_part()
    }

    /// The steps with an app's part can no longer be undone or redone: the
    /// history ends at the nearest of them either way, as
    /// [`crate::TextBox::forget_parts`]
    pub fn forget_parts(&mut self) {
        self.history.cut_at_parts();
    }

    /// Does `action`, keeping the history. A step undone or redone that has
    /// the app's part hands it back, for the app to do its side
    pub fn perform(&mut self, action: Action) -> Option<Turn<P>> {
        match action {
            Action::Edit(edit) => self.edit(edit),
            Action::Undo if self.can_undo() => {
                let now = self.line();
                let (before, part) = self.history.undo(now).expect("a step to undo");
                self.restore(before);
                return part.map(Turn::Undone);
            }
            Action::Redo if self.can_redo() => {
                let now = self.line();
                let (after, part) = self.history.redo(now).expect("a step to redo");
                self.restore(after);
                return part.map(Turn::Redone);
            }
            Action::Undo | Action::Redo => {}
        }
        None
    }

    /// The field's edit: part of the run going on, or a step of its own
    fn edit(&mut self, edit: Edit) {
        let Edit {
            value,
            before,
            after,
            by,
            remember,
        } = edit;
        if value == self.value {
            // A key that changed nothing (Backspace at the start) is no run
            self.span = after;
            self.run = None;
            return;
        }
        if !remember || self.password {
            self.value = value;
            self.span = after;
            return;
        }
        // The caret moved since the last edit: the run going on has ended
        if before != self.span {
            self.run = None;
        }
        let selected = before.selects();
        let run = match by {
            By::Typed(c) => Some(Run::Typing {
                space: c.is_whitespace(),
            }),
            // Deleting a selection is a step of its own
            By::Deleted if !selected => Some(Run::Deleting),
            By::Deleted | By::Other => None,
        };
        if !Run::joins(self.run, run, selected) {
            let text = std::mem::take(&mut self.value);
            self.history.record(Line { text, span: before }, None);
        }
        self.value = value;
        self.span = after;
        self.run = run;
    }

    /// The value set by the app and the field asked to put its caret at
    /// `caret`; the state before, unless this is a password
    fn set(&mut self, text: &str, caret: Caret) -> Option<Line> {
        let before = self.line();
        let value = Value::new(text);
        let at = |column: usize| value.grapheme_index_at_byte(column.min(text.len()));
        let span = match caret {
            Caret::Start => Span::at(0),
            Caret::End => Span::at(value.len()),
            Caret::At(cursor) => Span {
                anchor: at(cursor.selection.unwrap_or(cursor.position).column),
                caret: at(cursor.position.column),
            },
        };
        self.value = text.to_string();
        self.put(span);
        self.run = None;
        (!self.password).then_some(before)
    }

    fn line(&self) -> Line {
        Line {
            text: self.value.clone(),
            span: self.span,
        }
    }

    fn restore(&mut self, line: Line) {
        self.value = line.text;
        self.put(line.span);
        self.run = None;
    }

    /// The caret is to go to `span`: the field puts it there when next built
    fn put(&mut self, span: Span) {
        self.span = span;
        self.put = Some((REQUESTS.fetch_add(1, Ordering::Relaxed), span));
    }
}

impl<P: Clone> Undoable for ShortTextBox<P> {
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

/// What libcosmic's input says, inside a [`Field`]
#[derive(Clone, Debug)]
enum Change {
    Input(String),
    Submit,
    Clear,
    Toggle,
}

/// Which of libcosmic's inputs a field is
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Text,
    Search,
    Secure,
}

impl Kind {
    /// libcosmic's style for it
    fn style(self) -> cosmic::theme::TextInput {
        match self {
            Kind::Text | Kind::Secure => cosmic::theme::TextInput::Default,
            Kind::Search => cosmic::theme::TextInput::Search,
        }
    }

    /// libcosmic's style for it, with the look of a kept selection when
    /// that is drawn (as focused, so that libcosmic draws the selection)
    fn with_kept_selection(self) -> cosmic::theme::TextInput {
        cosmic::theme::TextInput::Custom {
            active: Box::new(move |theme| theme.active(&self.style())),
            error: Box::new(move |theme| theme.error(&self.style())),
            hovered: Box::new(move |theme| theme.hovered(&self.style())),
            focused: Box::new(move |theme| match kept::drawing() {
                Some(hovered) => self.kept_look(theme, hovered),
                None => theme.focused(&self.style()),
            }),
            disabled: Box::new(move |theme| theme.disabled(&self.style())),
        }
    }

    /// The field unfocused, as it is with the pointer over it or not, with
    /// a kept selection's colours
    fn kept_look(self, theme: &Theme, hovered: bool) -> Appearance {
        let style = self.style();
        let mut look = match hovered {
            true => theme.hovered(&style),
            false => theme.active(&style),
        };
        let container = theme.current_container();
        let text = look.text_color.unwrap_or_else(|| container.on.into());
        let beneath = kept::flat(look.background, container.base.into());
        (look.selected_fill, look.selected_text_color) =
            kept::kept_selection_colours(theme, text, beneath);
        look
    }
}

/// A short text box shown: libcosmic's text input over a
/// [`ShortTextBox`], publishing its [`Action`]s; made by [`text`],
/// [`search`] or [`secure`]. Wrap it in [`crate::text_menu`] for its menu,
/// with [`crate::TextMenuHistory::of`] the box for Undo · Redo.
pub struct Field<'a, M> {
    input: TextInput<'a, Change>,
    messages: Messages<'a, M>,
    /// Where the box asks for the caret to go, and the request's number
    put: Option<(u64, Span)>,
    /// What the input's overlays publish, made when they are asked for
    overlay_map: Option<Box<dyn Fn(Change) -> M + 'a>>,
}

/// The app's messages for what a field's input says
struct Messages<'a, M> {
    on_action: Rc<dyn Fn(Action) -> M + 'a>,
    on_submit: Option<M>,
    on_clear: Option<M>,
    on_toggle: Option<M>,
    /// Whether what is typed may be kept for Undo: not in a password
    remember: bool,
}

impl<M: Clone> Clone for Messages<'_, M> {
    fn clone(&self) -> Self {
        Self {
            on_action: self.on_action.clone(),
            on_submit: self.on_submit.clone(),
            on_clear: self.on_clear.clone(),
            on_toggle: self.on_toggle.clone(),
            remember: self.remember,
        }
    }
}

impl<M: Clone> Messages<'_, M> {
    /// The app's message for what the input said, the caret `before` and
    /// `after` it, made `by` a key
    fn message(&self, change: Change, before: Span, after: Span, by: By) -> M {
        let given = |message: &Option<M>| message.clone().expect("only asked for when given");
        match change {
            Change::Input(value) => (self.on_action)(Action::Edit(Edit {
                value,
                before,
                after,
                by,
                remember: self.remember,
            })),
            Change::Submit => given(&self.on_submit),
            Change::Clear => given(&self.on_clear),
            Change::Toggle => given(&self.on_toggle),
        }
    }
}

/// A short text box, as libcosmic's `text_input`
pub fn text<'a, P: Clone, M>(
    text_box: &'a ShortTextBox<P>,
    placeholder: impl Into<Cow<'a, str>>,
    on_action: impl Fn(Action) -> M + 'a,
) -> Field<'a, M> {
    let input = text_input::text_input(placeholder, text_box.value.as_str());
    Field::new(input, Kind::Text, text_box, on_action)
}

/// A search box, as libcosmic's `search_input`: its clear button is shown
/// with [`Field::on_clear`]
pub fn search<'a, P: Clone, M>(
    text_box: &'a ShortTextBox<P>,
    placeholder: impl Into<Cow<'a, str>>,
    on_action: impl Fn(Action) -> M + 'a,
) -> Field<'a, M> {
    let input = text_input::search_input(placeholder, text_box.value.as_str());
    Field::new(input, Kind::Search, text_box, on_action)
}

/// A password box, as libcosmic's `secure_input`: hidden while `hidden`,
/// with its show-or-hide button publishing `on_toggle`, if given. It keeps
/// no history: make its box with [`ShortTextBox::password`]
pub fn secure<'a, P: Clone, M>(
    text_box: &'a ShortTextBox<P>,
    placeholder: impl Into<Cow<'a, str>>,
    on_action: impl Fn(Action) -> M + 'a,
    hidden: bool,
    on_toggle: Option<M>,
) -> Field<'a, M> {
    let toggle = on_toggle.as_ref().map(|_| Change::Toggle);
    let input = text_input::secure_input(placeholder, text_box.value.as_str(), toggle, hidden);
    let mut field = Field::new(input, Kind::Secure, text_box, on_action);
    field.messages.on_toggle = on_toggle;
    field
}

impl<'a, M> Field<'a, M> {
    fn new<P: Clone>(
        input: TextInput<'a, Change>,
        kind: Kind,
        text_box: &ShortTextBox<P>,
        on_action: impl Fn(Action) -> M + 'a,
    ) -> Self {
        Self {
            input: input
                .on_input(Change::Input)
                .style(kind.with_kept_selection()),
            messages: Messages {
                on_action: Rc::new(on_action),
                on_submit: None,
                on_clear: None,
                on_toggle: None,
                remember: kind != Kind::Secure,
            },
            put: text_box.put,
            overlay_map: None,
        }
    }

    /// Enter publishes `message`
    pub fn on_submit(mut self, message: M) -> Self {
        self.input = self.input.on_submit(|_| Change::Submit);
        self.messages.on_submit = Some(message);
        self
    }

    /// A search box's clear button, shown while it has text, publishes
    /// `message`: the app clears the box ([`ShortTextBox::replace`], a step
    /// Undo takes back)
    pub fn on_clear(mut self, message: M) -> Self {
        self.input = self.input.on_clear(Change::Clear);
        self.messages.on_clear = Some(message);
        self
    }

    pub fn id(mut self, id: Id) -> Self {
        self.input = self.input.id(id);
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.input = self.input.width(width);
        self
    }

    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.input = self.input.padding(padding);
        self
    }

    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.input = self.input.size(size);
        self
    }

    /// `run` on the input, with what it says turned into the app's
    /// messages, each with where the caret was before and after
    fn forward(
        &mut self,
        input: &mut Tree,
        by: By,
        shell: &mut Shell<'_, M>,
        run: impl FnOnce(&mut TextInput<'a, Change>, &mut Tree, &mut Shell<'_, Change>),
    ) where
        M: Clone,
    {
        let before = span_of(input);
        let mut changes = Vec::new();
        let mut own = Shell::new(&mut changes);
        run(&mut self.input, input, &mut own);
        let after = span_of(input);
        shell.merge(own, |change| {
            self.messages.message(change, before, after, by)
        });
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
    match state.cursor().state(&state.tracked_value) {
        cursor::State::Index(at) => Span::at(at),
        cursor::State::Selection { start, end } => Span {
            anchor: start,
            caret: end,
        },
    }
}

/// What a key press does to steps: a character typed, a character deleted,
/// or something else (with Ctrl, a word deleted, a paste, a cut)
fn by_key(event: &Event) -> By {
    let Event::Keyboard(keyboard::Event::KeyPressed {
        key,
        text,
        modifiers,
        ..
    }) = event
    else {
        return By::Other;
    };
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return By::Other;
    }
    match key {
        Key::Named(Named::Backspace) => By::Deleted,
        // Shift+Delete is Cut
        Key::Named(Named::Delete) if !modifiers.shift() => By::Deleted,
        _ => text
            .as_ref()
            .and_then(|text| text.chars().find(|c| !c.is_control()))
            .map_or(By::Other, By::Typed),
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

impl<M> Field<'_, M> {
    fn as_input(&self) -> &dyn Widget<Change, Theme, Renderer> {
        &self.input
    }

    /// The tree a kept selection is drawn from, made or let go as the input
    /// is now. libcosmic's input keeps its selection unfocused only when a
    /// press elsewhere took the focus (Esc and Tab clear it), and its state
    /// cannot be focused again without taking the focus from another input,
    /// so the state is the one copied just before that press
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
        // The value changed since (the app set it): the selection drawn
        // would be of the old one
        if own.kept.as_ref().is_some_and(|kept| {
            state(kept).tracked_value != input.tracked_value
                || state(kept).cursor() != input.cursor()
        }) {
            own.kept = None;
        }
    }
}

impl<'a, M: Clone + 'a> Widget<M, Theme, Renderer> for Field<'a, M> {
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
        tree.children[0].diff(&mut self.input as &mut dyn Widget<Change, Theme, Renderer>);
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
        Widget::<Change, Theme, Renderer>::size(&self.input)
    }

    fn size_hint(&self) -> Size<Length> {
        Widget::<Change, Theme, Renderer>::size_hint(&self.input)
    }

    // The input carries the field's identity
    fn id(&self) -> Option<Id> {
        None
    }

    fn set_id(&mut self, id: Id) {
        Widget::<Change, Theme, Renderer>::set_id(&mut self.input, id);
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
        // key goes on to the app (its own Ctrl+Z)
        if focused
            && let Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                physical_key,
                modifiers,
                ..
            }) = event
            && let Some(action) = history_key(key, *physical_key, *modifiers)
        {
            if self.messages.remember {
                let action = match action {
                    text_box::Action::Undo => Action::Undo,
                    _ => Action::Redo,
                };
                shell.publish((self.messages.on_action)(action));
            }
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
        let at = Span::at(span_of(&tree.children[0]).caret);
        let messages = self.messages.clone();
        let map = self.overlay_map.insert(Box::new(move |change| {
            messages.message(change, at, at, By::Other)
        }));
        let map: &'b dyn Fn(Change) -> M = &**map;
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

impl<M: Clone> TextMenuTarget<M> for Field<'_, M> {
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

impl<'a, M: Clone + 'a> From<Field<'a, M>> for Element<'a, M> {
    fn from(field: Field<'a, M>) -> Self {
        Element::new(field)
    }
}
