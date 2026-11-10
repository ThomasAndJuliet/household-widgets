// SPDX-License-Identifier: GPL-3.0-only

//! The short text box: "Yes we should have a "short text box" widget and
//! much of the code can be shared between the text box and short text box
//! but there are also obviously different" (Juliet, 8 Nov), with "the usual
//! functionality of such a box which includes ctrl + z" (6 Nov); and the
//! selection either box keeps while unfocused, drawn dimmed.
//!
//! The box's steps are those of [`crate::TextBox`], so the cases here
//! follow `text_box_tests.rs`'s; the widget is driven through a real
//! `UserInterface`, and what is drawn is read from iced's headless renderer.

use std::sync::Arc;

use cosmic::cosmic_theme::{ThemeBuilder, palette::Srgb as Palette};
use cosmic::iced::advanced::clipboard;
use cosmic::iced::advanced::renderer::{Headless, Style};
use cosmic::iced::event::Status;
use cosmic::iced::keyboard::key::{Code, Physical};
use cosmic::iced::keyboard::{self, Key, Location, Modifiers};
use cosmic::iced::runtime::user_interface::{Cache, UserInterface};
use cosmic::iced::{Color, Event, Font, Length, Pixels, Point, Size, mouse};
use cosmic::widget::text_editor::{Cursor, Position};
use cosmic::{Element, Theme, widget};
use household_tones::{Srgb, tint};

use crate::Look;
use crate::Undoable;
use crate::short_text_box::Caret;
use crate::short_text_box::{self, Action, By, Edit, PasswordBox, ShortTextBox, Span};
use crate::text_box::{self, Turn};
use crate::{TextBox, TextMenuHistory};

fn span(anchor: usize, caret: usize) -> Span {
    Span { anchor, caret }
}

/// `text` typed into `short` a character at a time at its caret, as the
/// field reports each key
fn typed<P: Clone>(short: &mut ShortTextBox<P>, text: &str) {
    for c in text.chars() {
        let at = short.span.caret;
        let mut value: Vec<char> = short.value().chars().collect();
        value.insert(at, c);
        edited(
            short,
            &value.into_iter().collect::<String>(),
            span(at, at),
            span(at + 1, at + 1),
            By::Typed {
                space: c.is_whitespace(),
            },
        );
    }
}

/// An edit made `by` a key, from `before` to `after`, leaving `value`
fn edited<P: Clone>(short: &mut ShortTextBox<P>, value: &str, before: Span, after: Span, by: By) {
    short.perform(Action::Edit(Edit {
        value: value.into(),
        before,
        after,
        by,
        hidden: false,
    }));
}

/// Undo pressed until nothing is left: the value after each
fn undone<P: Clone>(short: &mut ShortTextBox<P>) -> Vec<String> {
    let mut values = Vec::new();
    while short.can_undo() {
        short.perform(Action::Undo);
        values.push(short.value().to_string());
    }
    values
}

#[test]
fn a_run_of_typing_is_one_step_and_a_space_after_a_word_starts_the_next() {
    let mut short: ShortTextBox = ShortTextBox::new();
    assert!(!short.can_undo() && !short.can_redo());
    typed(&mut short, "hello  world again");
    assert_eq!(undone(&mut short), ["hello  world", "hello", ""]);
    for want in ["hello", "hello  world", "hello  world again"] {
        short.perform(Action::Redo);
        assert_eq!(short.value(), want);
    }
    assert!(!short.can_redo());
}

#[test]
fn moving_the_caret_or_changing_between_typing_and_deleting_ends_a_step() {
    let mut short: ShortTextBox = ShortTextBox::new();
    typed(&mut short, "abd");
    // The caret moved left before the next key: its `before` is not where
    // the last edit left it
    edited(
        &mut short,
        "abcd",
        span(2, 2),
        span(3, 3),
        By::Typed { space: false },
    );
    edited(&mut short, "abd", span(3, 3), span(2, 2), By::Deleted);
    edited(&mut short, "ad", span(2, 2), span(1, 1), By::Deleted);
    typed(&mut short, "xy");
    assert_eq!(short.value(), "axyd");
    assert_eq!(undone(&mut short), ["ad", "abcd", "abd", ""]);
}

#[test]
fn a_paste_a_word_deleted_and_deleting_a_selection_are_a_step_each() {
    let mut short: ShortTextBox = ShortTextBox::new();
    typed(&mut short, "ab");
    edited(&mut short, "abXY", span(2, 2), span(4, 4), By::Other);
    typed(&mut short, "c");
    edited(&mut short, "abX", span(5, 3), span(3, 3), By::Deleted);
    edited(&mut short, "", span(3, 3), span(0, 0), By::Other);
    assert_eq!(undone(&mut short), ["abX", "abXYc", "abXY", "ab", ""]);
}

#[test]
fn typing_over_a_selection_is_one_step_with_the_typing_after_it() {
    let mut short: ShortTextBox = ShortTextBox::starting_with("old");
    edited(
        &mut short,
        "n",
        span(0, 3),
        span(1, 1),
        By::Typed { space: false },
    );
    typed(&mut short, "ew");
    assert_eq!(short.value(), "new");
    assert_eq!(undone(&mut short), ["old"]);
}

#[test]
fn a_key_that_changes_nothing_is_no_step_and_joins_none() {
    let mut short: ShortTextBox = ShortTextBox::new();
    edited(&mut short, "", span(0, 0), span(0, 0), By::Deleted);
    assert!(!short.can_undo());
    typed(&mut short, "a");
    edited(&mut short, "a", span(0, 0), span(0, 0), By::Deleted);
    edited(&mut short, "", span(0, 0), span(0, 0), By::Deleted);
    assert_eq!(undone(&mut short), ["a", ""]);
}

#[test]
fn the_apps_change_is_one_step_and_where_the_field_starts_is_none() {
    let mut short: ShortTextBox = ShortTextBox::starting_with("draft");
    assert!(!short.can_undo(), "a new box has nothing to undo");
    // Clearing a search: one step Ctrl+Z takes back
    short.replace("", Caret::End);
    assert_eq!(short.value(), "");
    assert_eq!(undone(&mut short), ["draft"]);
    // The same value again is no step
    short.replace("draft", Caret::End);
    assert!(!short.can_undo());
}

#[test]
fn undo_asks_the_field_to_put_back_the_caret_and_selection() {
    let mut short: ShortTextBox = ShortTextBox::starting_with("hello world");
    edited(
        &mut short,
        "x",
        span(0, 11),
        span(1, 1),
        By::Typed { space: false },
    );
    short.perform(Action::Undo);
    assert_eq!(short.value(), "hello world");
    let put = |short: &ShortTextBox| short.put.map(|(_, span)| span);
    assert_eq!(
        put(&short),
        Some(span(0, 11)),
        "the selection typed over is selected again"
    );
    short.perform(Action::Redo);
    assert_eq!(put(&short), Some(span(1, 1)));
}

#[test]
fn a_step_with_the_apps_part_hands_it_back() {
    let mut short: ShortTextBox<&str> = ShortTextBox::new();
    typed(&mut short, "query");
    short.replace_with("", Caret::End, "saved search");
    assert_eq!(short.undo_part(), Some(&"saved search"));
    assert_eq!(
        short.perform(Action::Undo),
        Some(Turn::Undone("saved search"))
    );
    assert_eq!(short.value(), "query");
    assert_eq!(
        short.perform(Action::Redo),
        Some(Turn::Redone("saved search"))
    );
    short.forget_parts();
    assert!(!short.can_undo(), "the history ends at the part");
}

/// A password's box has no history to undo, and writes nothing out. A
/// plain box cannot be shown as a password, nor a password in a plain
/// field: `secure` takes only a PasswordBox, the other kinds never one
#[test]
fn a_password_keeps_no_history() {
    let mut password = PasswordBox::new();
    let edit = |value: &str| {
        Action::Edit(Edit {
            value: value.into(),
            before: span(0, 0),
            after: span(value.len(), value.len()),
            by: By::Typed { space: false },
            hidden: true,
        })
    };
    password.perform(edit("hunter2"));
    password.perform(Action::Undo);
    assert_eq!(password.value(), "hunter2", "Undo does nothing");
    password.set("swordfish", Caret::End);
    assert_eq!(&*password, "swordfish");
    assert!(
        !format!("{password:?}").contains("swordfish"),
        "a password is not written out"
    );
}

/// A change leaves nothing of the password it replaces in the box's
/// buffer: the old value is zeroed where it was, the new one written over
/// the start of it (what the allocator was given back, before the buffer
/// grew, was zeroed too, but cannot be read once given back)
#[test]
fn a_passwords_change_leaves_nothing_of_the_old_one_in_its_buffer() {
    let password_edit = |value: &str| {
        Action::Edit(Edit {
            value: value.into(),
            before: span(0, 0),
            after: span(value.len(), value.len()),
            by: By::Typed { space: false },
            hidden: true,
        })
    };
    let mut password = PasswordBox::new();
    password.perform(password_edit("correct horse battery staple"));
    // (shorter: written in the same buffer, which every change zeroes
    // first, so all of it can be read now)
    password.perform(password_edit("tr0ub4dor"));
    let buffer = password.buffer();
    assert_eq!(&buffer[..9], b"tr0ub4dor");
    assert!(
        buffer[9..].iter().all(|byte| *byte == 0),
        "the old password is left in the buffer: {buffer:?}"
    );
    // The app's change too
    password.set("x", Caret::End);
    let buffer = password.buffer();
    assert_eq!(buffer[0], b'x');
    assert!(buffer[1..].iter().all(|byte| *byte == 0));
}

/// take() hands the password over: the box is empty, still writes nothing
/// out, and can be typed into again
#[test]
fn a_password_taken_leaves_its_box_empty() {
    let mut password = PasswordBox::new();
    password.set("swordfish", Caret::End);
    assert_eq!(password.take(), "swordfish");
    assert_eq!(password.value(), "");
    assert_eq!(password.take(), "", "nothing is left to take");
    password.set("hunter2", Caret::End);
    assert_eq!(password.value(), "hunter2");
    let written = format!("{password:?}");
    assert!(!written.contains("hunter2"), "{written}");
    assert!(written.contains("(hidden)"), "{written}");
}

/// A sensitive box undoes and redoes as any box does, and writes nothing
/// of its value out: neither the box nor its field's edits
#[test]
fn a_sensitive_box_undoes_as_ever_and_writes_nothing_out() {
    let mut plain: ShortTextBox = ShortTextBox::new();
    let mut sensitive: ShortTextBox = ShortTextBox::sensitive();
    assert!(sensitive.is_sensitive() && !plain.is_sensitive());
    for short in [&mut plain, &mut sensitive] {
        typed(short, "Synthetic Person");
        short.replace("4/29", Caret::End);
    }
    assert_eq!(sensitive.value(), "4/29");
    assert_eq!(
        undone(&mut sensitive),
        undone(&mut plain),
        "the same steps back"
    );
    sensitive.perform(Action::Redo);
    plain.perform(Action::Redo);
    assert_eq!(sensitive.value(), plain.value(), "and the same forward");
    assert!(!sensitive.value().is_empty());
    let written = format!("{sensitive:?}");
    assert!(
        !written.contains("Synthetic") && written.contains("(hidden)"),
        "{written}"
    );
    // Its field's edits are not written out either; a plain box's are
    let mut window = Window::new();
    let (messages, _) = window.send(field(&sensitive), &click(IN_FIELD));
    apply(&mut sensitive, messages);
    let (messages, _) = window.send(
        field(&sensitive),
        &[press("q", Code::KeyQ, Modifiers::empty())],
    );
    let written = format!("{messages:?}");
    assert!(!written.contains("Synthetic"), "{written}");
    let edits = apply(&mut sensitive, messages);
    assert_eq!(edits.len(), 1, "the field still reports its edit");
    // and Ctrl+Z is the field's, published for the box to undo
    let (messages, _) = window.send(
        field(&sensitive),
        &[press("z", Code::KeyZ, Modifiers::CTRL)],
    );
    assert_eq!(messages, [Message::Box(Action::Undo)]);
}

fn shown_password(password: &PasswordBox) -> short_text_box::Field<'_, Message> {
    short_text_box::shown(password, "", Message::Box)
}

/// A password shown as a plain field is drawn as a text box is, the same
/// size, and keeps a password's ways: Ctrl+Z taken and nothing undone,
/// its edits not written out
#[test]
fn a_password_shown_plain_is_a_text_fields_size_and_a_passwords_box() {
    let mut window = Window::new();
    let mut password = PasswordBox::new();
    password.set("generated-secret", Caret::End);
    let plain: ShortTextBox = ShortTextBox::starting_with("generated-secret");
    assert_eq!(
        window.size_of(shown_password(&password)),
        window.size_of(field(&plain)),
        "the size a text box is"
    );
    let (messages, _) = window.send(shown_password(&password), &click(IN_FIELD));
    for message in messages {
        if let Message::Box(action) = message {
            password.perform(action);
        }
    }
    let (messages, _) = window.send(
        shown_password(&password),
        &[press("q", Code::KeyQ, Modifiers::empty())],
    );
    assert!(
        !format!("{messages:?}").contains("generated-secret"),
        "{messages:?}"
    );
    for message in messages {
        if let Message::Box(action) = message {
            password.perform(action);
        }
    }
    let (messages, statuses) = window.send(
        shown_password(&password),
        &[press("z", Code::KeyZ, Modifiers::CTRL)],
    );
    assert!(messages.is_empty(), "no history to undo: {messages:?}");
    assert_eq!(statuses, [Status::Captured]);
}

/// Keys as a window can give them, many at once before the app answers
/// (typing fast, a VNC session): the input edits its own copy of the value
/// with each, so a character the field does not take must be refused as it
/// comes, not by the app afterwards
#[test]
fn a_field_refuses_what_it_does_not_take_however_fast_it_comes() {
    fn pick(short: &ShortTextBox, accepting: bool) -> short_text_box::Field<'_, Message> {
        match accepting {
            true => field(short).accepts(|c| !c.is_whitespace()),
            false => field(short),
        }
    }
    let keys: Vec<(Event, Point)> = "alpha beta"
        .chars()
        .map(|c| {
            let letter = c.to_string();
            key(
                Key::Character(letter.clone().into()),
                Code::KeyA,
                Modifiers::empty(),
                Some(&letter),
            )
        })
        .collect();
    for (accepting, want) in [(true, "alphabeta"), (false, "alpha beta")] {
        let mut window = Window::new();
        let mut short: ShortTextBox = ShortTextBox::new();
        let (messages, _) = window.send(pick(&short, accepting), &click(IN_FIELD));
        apply(&mut short, messages);
        // All ten keys at once, before the app has answered any
        let (messages, statuses) = window.send(pick(&short, accepting), &keys);
        apply(&mut short, messages);
        assert_eq!(short.value(), want, "taking only some: {accepting}");
        assert_eq!(statuses.len(), 10);
    }
}

/// A field at fault is drawn in libcosmic's error look, in place of the
/// household border: one border, libcosmic's error colour, its error fill,
/// at rest and with the keys (rule 25a: "error borders keep libcosmic's")
#[test]
fn a_field_at_fault_has_libcosmics_error_look_and_one_border() {
    let theme = theme();
    let short: ShortTextBox = ShortTextBox::starting_with("one");
    let error = rgb8(theme.cosmic().destructive_color().into());
    let rest = rgb8(
        Look::normal()
            .border(&theme, crate::look::State::Rest)
            .color,
    );
    let ring = rgb8(
        Look::normal()
            .border(&theme, crate::look::State::Focused)
            .color,
    );
    let inside = Point::new(IN_FIELD.x + 60.0, IN_FIELD.y);
    for focused in [false, true] {
        let mut window = Window::new();
        if focused {
            let _ = window.send(field(&short).invalid(true), &click(IN_FIELD));
        }
        let shot = window.shot(field(&short).invalid(true), &theme);
        // libcosmic's own input in its error state, for what its look is
        let mut libcosmic = Window::new();
        let input = || {
            widget::text_input("", "one")
                .on_input(|_| Message::Submit)
                .error("")
        };
        if focused {
            let _ = libcosmic.send(input(), &click(IN_FIELD));
        }
        let expected = libcosmic.shot(input(), &theme);
        let all = 0..WINDOW.height as usize;
        let border = shot.count_in(all.clone(), error);
        assert!(
            border > 20,
            "the error colour's border ({focused}): {border}"
        );
        assert_eq!(
            border,
            expected.count_in(all.clone(), error),
            "the one border libcosmic draws ({focused})"
        );
        // (no household border beside it: no more of its colours than
        // libcosmic's own error input draws)
        for (colour, what) in [(rest, "rest border"), (ring, "focus ring")] {
            assert_eq!(
                shot.count_in(all.clone(), colour),
                expected.count_in(all.clone(), colour),
                "no household {what} beside it ({focused})"
            );
        }
        assert_eq!(
            shot.at(inside),
            expected.at(inside),
            "libcosmic's error fill ({focused})"
        );
    }
}

#[test]
fn the_menu_offers_a_short_boxs_undo_and_redo() {
    let mut short: ShortTextBox = ShortTextBox::new();
    let history = TextMenuHistory::of(&short, |action| action);
    assert!(!history.can_undo && !history.can_redo);
    assert_eq!((history.undo, history.redo), (Action::Undo, Action::Redo));
    typed(&mut short, "a");
    assert!(TextMenuHistory::of(&short, |action| action).can_undo);
}

// ---------------------------------------------------------------------------
// The widget
// ---------------------------------------------------------------------------

const WINDOW: Size = Size::new(320.0, 120.0);
/// Inside the field, at the window's top, and below it
const IN_FIELD: Point = Point::new(150.0, 16.0);
const BELOW: Point = Point::new(150.0, 100.0);

#[derive(Clone, Debug, PartialEq)]
enum Message {
    Box(Action),
    Submit,
    Pasted(String),
}

/// A clipboard holding `0`
#[derive(Default)]
struct Board(Option<String>);

impl clipboard::Clipboard for Board {
    fn read(&self, _: clipboard::Kind) -> Option<String> {
        self.0.clone()
    }

    fn write(&mut self, _: clipboard::Kind, text: String) {
        self.0 = Some(text);
    }
}

/// A window kept between builds, as the event loop keeps it
struct Window {
    renderer: cosmic::Renderer,
    cache: Cache,
    board: Board,
}

impl Window {
    fn new() -> Self {
        let renderer = cosmic::iced::futures::executor::block_on(
            <cosmic::Renderer as Headless>::new(Font::default(), Pixels(14.0), Some("tiny-skia")),
        )
        .expect("a headless renderer");
        Self {
            renderer,
            cache: Cache::default(),
            board: Board::default(),
        }
    }

    /// `view` built, given each event with the pointer where it says; what
    /// was published and whether each event was captured
    fn send<'a, M: 'a>(
        &mut self,
        view: impl Into<Element<'a, M>>,
        events: &[(Event, Point)],
    ) -> (Vec<M>, Vec<Status>) {
        let element = page(view.into());
        let cache = std::mem::take(&mut self.cache);
        let mut ui = UserInterface::build(element, WINDOW, cache, &mut self.renderer);
        let mut messages = Vec::new();
        let mut statuses = Vec::new();
        for (event, at) in events {
            let (_, status) = ui.update(
                std::slice::from_ref(event),
                mouse::Cursor::Available(*at),
                &mut self.renderer,
                &mut self.board,
                &mut messages,
            );
            statuses.extend(status);
        }
        self.cache = ui.into_cache();
        (messages, statuses)
    }

    /// The size `view`'s text field is laid out at
    fn size_of<'a, M: 'a>(&mut self, view: impl Into<Element<'a, M>>) -> Size {
        use cosmic::iced::advanced::widget::{Id, Operation, operation};
        struct Find(Option<Size>);
        impl Operation for Find {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                operate(self);
            }
            fn text_input(
                &mut self,
                _: Option<&Id>,
                bounds: cosmic::iced::Rectangle,
                _: &mut dyn operation::TextInput,
            ) {
                self.0.get_or_insert(bounds.size());
            }
        }
        let mut ui = UserInterface::build(
            page(view.into()),
            WINDOW,
            Cache::default(),
            &mut self.renderer,
        );
        let mut find = Find(None);
        ui.operate(&self.renderer, &mut find);
        find.0.expect("a text field")
    }

    /// `view` drawn in `theme`, the pointer away from it
    fn shot<'a, M: 'a>(&mut self, view: impl Into<Element<'a, M>>, theme: &Theme) -> Shot {
        self.shot_at(view, theme, BELOW)
    }

    /// `view` drawn in `theme`, the pointer at `pointer`
    fn shot_at<'a, M: 'a>(
        &mut self,
        view: impl Into<Element<'a, M>>,
        theme: &Theme,
        pointer: Point,
    ) -> Shot {
        let element = page(view.into());
        let cache = std::mem::take(&mut self.cache);
        let mut ui = UserInterface::build(element, WINDOW, cache, &mut self.renderer);
        // The pointer there, and the frame asked for, as the event loop
        // does before drawing: an editor takes its state's look from it
        let redraw = Event::Window(cosmic::iced::window::Event::RedrawRequested(
            std::time::Instant::now(),
        ));
        let _ = ui.update(
            &[
                Event::Mouse(mouse::Event::CursorMoved { position: pointer }),
                redraw,
            ],
            mouse::Cursor::Available(pointer),
            &mut self.renderer,
            &mut self.board,
            &mut Vec::new(),
        );
        let style = Style {
            text_color: theme.cosmic().on_bg_color().into(),
            ..Style::default()
        };
        ui.draw(
            &mut self.renderer,
            theme,
            &style,
            mouse::Cursor::Available(pointer),
        );
        self.cache = ui.into_cache();
        Shot(self.renderer.screenshot(
            cosmic::iced::Size::new(WINDOW.width as u32, WINDOW.height as u32),
            1.0,
            Color::BLACK,
        ))
    }
}

/// `content` at the top of a window with the theme's background
fn page<'a, M: 'a>(content: Element<'a, M>) -> Element<'a, M> {
    widget::container(widget::column::with_children(vec![content]))
        .class(cosmic::theme::Container::Background)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

struct Shot(Vec<u8>);

impl Shot {
    fn at(&self, at: Point) -> [u8; 3] {
        let i = (at.y as usize * WINDOW.width as usize + at.x as usize) * 4;
        [self.0[i], self.0[i + 1], self.0[i + 2]]
    }

    /// How many pixels of the box's row (its top 32) are within 3 of `colour`
    fn count(&self, colour: [u8; 3]) -> usize {
        self.count_in(0..32, colour)
    }

    /// How many pixels of `rows` are within 3 of `colour`
    fn count_in(&self, rows: std::ops::Range<usize>, colour: [u8; 3]) -> usize {
        rows.flat_map(|y| (0..WINDOW.width as usize).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let pixel = self.at(Point::new(x as f32, y as f32));
                (0..3).all(|i| pixel[i].abs_diff(colour[i]) <= 3)
            })
            .count()
    }
}

fn click(at: Point) -> Vec<(Event, Point)> {
    vec![
        (Event::Mouse(mouse::Event::CursorMoved { position: at }), at),
        (
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            at,
        ),
        (
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            at,
        ),
    ]
}

fn key(key: Key, code: Code, modifiers: Modifiers, text: Option<&str>) -> (Event, Point) {
    (
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: Physical::Code(code),
            location: Location::Standard,
            modifiers,
            text: text.map(Into::into),
            repeat: false,
        }),
        IN_FIELD,
    )
}

/// A letter pressed: with Ctrl, a shortcut, with no text
fn press(letter: &str, code: Code, modifiers: Modifiers) -> (Event, Point) {
    let text = (!modifiers.control()).then_some(letter);
    key(Key::Character(letter.into()), code, modifiers, text)
}

fn named(named: keyboard::key::Named, code: Code) -> (Event, Point) {
    key(Key::Named(named), code, Modifiers::empty(), None)
}

fn modifiers(held: Modifiers) -> (Event, Point) {
    (
        Event::Keyboard(keyboard::Event::ModifiersChanged(held)),
        IN_FIELD,
    )
}

fn field(short: &ShortTextBox) -> short_text_box::Field<'_, Message> {
    short_text_box::text(short, "", Message::Box).on_submit(Message::Submit)
}

/// The box's actions among `messages`, done to it; its edits
fn apply(short: &mut ShortTextBox, messages: Vec<Message>) -> Vec<Edit> {
    let mut edits = Vec::new();
    for message in messages {
        if let Message::Box(action) = message {
            if let Action::Edit(edit) = &action {
                edits.push(edit.clone());
            }
            short.perform(action);
        }
    }
    edits
}

#[test]
fn ctrl_z_is_the_fields_only_while_it_is_focused() {
    let mut window = Window::new();
    let mut short: ShortTextBox = ShortTextBox::starting_with("hello");
    let ctrl_z = press("z", Code::KeyZ, Modifiers::CTRL);
    // Unfocused, the key goes on to the app (Marginalium's notes Undo)
    let (messages, statuses) = window.send(field(&short), std::slice::from_ref(&ctrl_z));
    assert!(messages.is_empty());
    assert_eq!(statuses, [Status::Ignored]);
    // Focused, it is the field's, and taken
    let (messages, _) = window.send(field(&short), &click(IN_FIELD));
    apply(&mut short, messages);
    let (messages, _) = window.send(field(&short), &[press("s", Code::KeyS, Modifiers::empty())]);
    apply(&mut short, messages);
    assert_eq!(short.value(), "hellos");
    for (keys, want) in [
        (ctrl_z.clone(), Action::Undo),
        (
            press("z", Code::KeyZ, Modifiers::CTRL | Modifiers::SHIFT),
            Action::Redo,
        ),
        (press("y", Code::KeyY, Modifiers::CTRL), Action::Redo),
    ] {
        let (messages, statuses) = window.send(field(&short), &[keys]);
        assert_eq!(messages, [Message::Box(want)]);
        assert_eq!(statuses, [Status::Captured]);
    }
}

#[test]
fn a_password_field_takes_ctrl_z_and_does_nothing() {
    let mut window = Window::new();
    let short = PasswordBox::new();
    let secure = |short| short_text_box::secure(short, "", Message::Box, true, None);
    window.send(secure(&short), &click(IN_FIELD));
    let (messages, statuses) =
        window.send(secure(&short), &[press("z", Code::KeyZ, Modifiers::CTRL)]);
    assert!(messages.is_empty(), "{messages:?}");
    assert_eq!(statuses, [Status::Captured]);
    let (messages, _) = window.send(
        secure(&short),
        &[press("a", Code::KeyA, Modifiers::empty())],
    );
    assert!(
        matches!(&messages[..], [Message::Box(Action::Edit(edit))] if edit.hidden),
        "what is typed is not to be kept: {messages:?}"
    );
}

#[test]
fn the_field_says_what_was_typed_and_where_and_enter_submits() {
    let mut window = Window::new();
    let mut short: ShortTextBox = ShortTextBox::new();
    let (messages, _) = window.send(field(&short), &click(IN_FIELD));
    apply(&mut short, messages);
    let (messages, _) = window.send(field(&short), &[press("a", Code::KeyA, Modifiers::empty())]);
    let edits = apply(&mut short, messages);
    assert_eq!(
        edits,
        [Edit {
            value: "a".into(),
            before: span(0, 0),
            after: span(1, 1),
            by: By::Typed { space: false },
            hidden: false,
        }]
    );
    let (messages, _) = window.send(
        field(&short),
        &[named(keyboard::key::Named::Backspace, Code::Backspace)],
    );
    let edits = apply(&mut short, messages);
    assert_eq!(
        edits.iter().map(|edit| edit.by).collect::<Vec<_>>(),
        [By::Deleted]
    );
    let (messages, _) = window.send(
        field(&short),
        &[named(keyboard::key::Named::Enter, Code::Enter)],
    );
    assert_eq!(messages, [Message::Submit]);
    assert_eq!(short.value(), "", "no new line");
}

#[test]
fn undo_puts_the_selection_back_in_the_field() {
    let mut window = Window::new();
    let mut short: ShortTextBox = ShortTextBox::starting_with("hello world");
    let (messages, _) = window.send(field(&short), &click(IN_FIELD));
    apply(&mut short, messages);
    let (messages, _) = window.send(field(&short), &[press("a", Code::KeyA, Modifiers::CTRL)]);
    apply(&mut short, messages);
    let (messages, _) = window.send(field(&short), &[press("x", Code::KeyX, Modifiers::empty())]);
    apply(&mut short, messages);
    assert_eq!(short.value(), "x");
    short.perform(Action::Undo);
    assert_eq!(short.value(), "hello world");
    // The field, built again, has the selection back: what is typed next
    // replaces it
    let (messages, _) = window.send(field(&short), &[press("y", Code::KeyY, Modifiers::empty())]);
    let edits = apply(&mut short, messages);
    assert_eq!(edits.len(), 1, "{edits:?}");
    assert_eq!(edits[0].before, span(0, 11));
    assert_eq!(short.value(), "y");
}

#[test]
fn shift_click_extends_the_selection_from_where_it_was() {
    let mut window = Window::new();
    let mut short: ShortTextBox = ShortTextBox::starting_with("hello world");
    // The caret at the end, by a click past the text
    let (messages, _) = window.send(field(&short), &click(Point::new(300.0, 16.0)));
    apply(&mut short, messages);
    let mut events = vec![modifiers(Modifiers::SHIFT)];
    events.extend(click(Point::new(1.0, 16.0)));
    events.push(modifiers(Modifiers::empty()));
    let (messages, _) = window.send(field(&short), &events);
    apply(&mut short, messages);
    let (messages, _) = window.send(field(&short), &[press("x", Code::KeyX, Modifiers::empty())]);
    let edits = apply(&mut short, messages);
    assert_eq!(edits.len(), 1, "{edits:?}");
    assert_eq!(
        edits[0].before,
        span(11, 0),
        "selected from the caret to the click"
    );
    assert_eq!(short.value(), "x");
}

// ---------------------------------------------------------------------------
// A selection kept while unfocused, drawn dimmed
// ---------------------------------------------------------------------------

/// The light theme with the accent the household's references use
fn theme() -> Theme {
    let accent = Palette::new(f32::from(0x7f_u8) / 255.0, f32::from(0x24_u8) / 255.0, 0.0);
    Theme::custom(Arc::new(ThemeBuilder::light().accent(accent).build()))
}

fn rgb8(c: Color) -> [u8; 3] {
    let [r, g, b, _] = c.into_rgba8();
    [r, g, b]
}

/// The accent, and the household's selection tint of it over `beneath`
fn accent_and_tint(theme: &Theme, beneath: [u8; 3]) -> ([u8; 3], [u8; 3]) {
    let accent = rgb8(theme.cosmic().accent_color().into());
    let [r, g, b] = accent;
    let [br, bg, bb] = beneath;
    let tinted = tint(Srgb::from_rgb8(r, g, b), Srgb::from_rgb8(br, bg, bb));
    (accent, tinted.to_rgb8())
}

/// A field with "hello world" all selected, drawn after `then`
fn selected_field(then: &[(Event, Point)], theme: &Theme) -> Shot {
    let mut window = Window::new();
    let short: ShortTextBox = ShortTextBox::starting_with("hello world");
    window.send(field(&short), &click(IN_FIELD));
    window.send(field(&short), &[press("a", Code::KeyA, Modifiers::CTRL)]);
    window.send(field(&short), then);
    window.shot(field(&short), theme)
}

#[test]
fn a_fields_selection_kept_unfocused_is_drawn_in_the_tint() {
    let theme = theme();
    // Focused: the solid accent
    let focused = selected_field(&[], &theme);
    // Past the text, the field's own background
    let beneath = focused.at(Point::new(300.0, 16.0));
    let (accent, tinted) = accent_and_tint(&theme, beneath);
    assert!(focused.count(accent) > 100, "a focused selection is solid");
    assert!(focused.count(tinted) < 10);
    // A press elsewhere (a menu opened) takes the focus, not the selection
    let kept = selected_field(&click(BELOW), &theme);
    assert!(
        kept.count(tinted) > 100,
        "the selection kept is drawn in the tint: {} pixels",
        kept.count(tinted)
    );
    assert!(kept.count(accent) < 10, "and not as a focused one");
    // Esc lets the selection go, as libcosmic does: nothing is drawn
    let gone = selected_field(&[named(keyboard::key::Named::Escape, Code::Escape)], &theme);
    assert!(gone.count(tinted) < 10 && gone.count(accent) < 10);
}

#[test]
fn a_text_boxs_selection_kept_unfocused_is_drawn_in_the_tint() {
    let theme = theme();
    let selected: TextBox = TextBox::starting_with(
        "hello world",
        text_box::Caret::At(Cursor {
            position: Position {
                line: 0,
                column: 11,
            },
            selection: Some(Position { line: 0, column: 0 }),
        }),
    );
    fn view(text_box: &TextBox) -> Element<'_, text_box::Action> {
        text_box::wrap(
            text_box::editor(text_box, Look::normal(), |action| action).height(32.0),
            text_box,
            |action| action,
        )
    }
    // Never focused (the app selected in it): nothing is kept, so nothing
    // drawn
    let mut window = Window::new();
    let never = window.shot(view(&selected), &theme);
    let beneath = never.at(Point::new(300.0, 16.0));
    let (accent, tinted) = accent_and_tint(&theme, beneath);
    assert!(never.count(tinted) < 10 && never.count(accent) < 10);
    // Focused (the click's own action not done, so the selection stays):
    // the solid accent
    window.send(view(&selected), &click(IN_FIELD));
    let focused = window.shot(view(&selected), &theme);
    assert!(focused.count(accent) > 100, "a focused selection is solid");
    assert!(focused.count(tinted) < 10);
    // A press elsewhere (a menu opened) takes the focus, not the selection
    window.send(view(&selected), &click(BELOW));
    let kept = window.shot(view(&selected), &theme);
    assert!(
        kept.count(tinted) > 100,
        "the selection kept is drawn in the tint: {} pixels",
        kept.count(tinted)
    );
    assert!(kept.count(accent) < 10);
    // Without a selection, nothing
    let none: TextBox = TextBox::starting_with("hello world", text_box::Caret::End);
    let mut window = Window::new();
    let plain = window.shot(view(&none), &theme);
    assert!(plain.count(tinted) < 10 && plain.count(accent) < 10);
}

#[test]
fn the_apps_same_value_again_moves_nothing() {
    let mut short: ShortTextBox = ShortTextBox::starting_with("term");
    edited(
        &mut short,
        "terms",
        span(4, 4),
        span(5, 5),
        By::Typed { space: false },
    );
    let before = short.put;
    // A reload putting the same term back: no step, and the caret left
    short.replace("terms", Caret::End);
    assert_eq!(short.put, before, "no caret request");
    assert_eq!(undone(&mut short), ["term"]);
}

#[test]
fn the_app_puts_the_caret_and_selection_by_graphemes() {
    let mut short: ShortTextBox = ShortTextBox::starting_with("");
    short.replace(
        "naïve café",
        Caret::At {
            anchor: 6,
            caret: 10,
        },
    );
    assert_eq!(short.put.map(|(_, at)| at), Some(span(6, 10)));
    short.replace(
        "abc",
        Caret::At {
            anchor: 1,
            caret: 99,
        },
    );
    assert_eq!(
        short.put.map(|(_, at)| at),
        Some(span(1, 3)),
        "kept inside the value"
    );
}

#[test]
fn a_passwords_edit_is_not_written_out() {
    let edit = Edit {
        value: "hunter2".into(),
        before: span(0, 0),
        after: span(7, 7),
        by: By::Other,
        hidden: true,
    };
    assert!(!format!("{edit:?}").contains("hunter2"));
}

#[test]
fn what_an_input_method_commits_is_typing_and_a_paste_is_told() {
    let mut window = Window::new();
    let mut short: ShortTextBox = ShortTextBox::new();
    fn view(short: &ShortTextBox) -> short_text_box::Field<'_, Message> {
        field(short).on_paste(Message::Pasted)
    }
    let (messages, _) = window.send(view(&short), &click(IN_FIELD));
    apply(&mut short, messages);
    let commit = Event::InputMethod(cosmic::iced::advanced::input_method::Event::Commit(
        "日本".into(),
    ));
    let (messages, _) = window.send(view(&short), &[(commit, IN_FIELD)]);
    let edits = apply(&mut short, messages);
    assert_eq!(edits.len(), 1, "{edits:?}");
    assert_eq!(edits[0].by, By::Typed { space: false });
    let (messages, _) = window.send(view(&short), &[press("x", Code::KeyX, Modifiers::empty())]);
    apply(&mut short, messages);
    assert_eq!(short.value(), "日本x");
    assert_eq!(undone(&mut short), [""], "one run of typing");
    // A paste is the box's edit, then the app's own message
    window.board = Board(Some("pasted".into()));
    let (messages, _) = window.send(view(&short), &[press("v", Code::KeyV, Modifiers::CTRL)]);
    assert!(
        matches!(&messages[..], [Message::Box(Action::Edit(edit)), Message::Pasted(text)]
            if edit.value == "pasted" && edit.by == By::Other && text == "pasted"),
        "{messages:?}"
    );
}

/// `short` in the field of `kind`
fn shown<'a>(short: &'a ShortTextBox, kind: &str) -> Element<'a, Message> {
    match kind {
        "inline" => short_text_box::inline(short, "", Message::Box).into(),
        _ => short_text_box::editable(short, "", Message::Box, true, |_| Message::Submit).into(),
    }
}

#[test]
fn the_inline_and_editable_kinds_keep_the_history_too() {
    for kind in ["inline", "editable"] {
        let mut window = Window::new();
        let mut short: ShortTextBox = ShortTextBox::starting_with("name");
        let (messages, _) = window.send(shown(&short, kind), &click(IN_FIELD));
        apply(&mut short, messages);
        let (messages, _) = window.send(
            shown(&short, kind),
            &[press("a", Code::KeyA, Modifiers::CTRL)],
        );
        apply(&mut short, messages);
        let (messages, _) = window.send(
            shown(&short, kind),
            &[press("x", Code::KeyX, Modifiers::empty())],
        );
        apply(&mut short, messages);
        assert_eq!(short.value(), "x", "{kind}");
        let (messages, statuses) = window.send(
            shown(&short, kind),
            &[press("z", Code::KeyZ, Modifiers::CTRL)],
        );
        assert_eq!(statuses, [Status::Captured], "{kind}");
        apply(&mut short, messages);
        assert_eq!(short.value(), "name", "{kind}");
    }
}

/// A field never focused keeps nothing: the app's selection in it is not
/// drawn as a kept one
#[test]
fn a_field_never_focused_draws_no_kept_selection() {
    let theme = theme();
    let mut window = Window::new();
    let mut short: ShortTextBox = ShortTextBox::starting_with("");
    window.send(field(&short), &[]);
    short.replace(
        "hello world",
        Caret::At {
            anchor: 0,
            caret: 11,
        },
    );
    window.send(field(&short), &[]);
    let shot = window.shot(field(&short), &theme);
    let (accent, tinted) = accent_and_tint(&theme, shot.at(Point::new(300.0, 16.0)));
    assert!(shot.count(tinted) < 10 && shot.count(accent) < 10);
}

/// The field's own menu: while it is open the field keeps the keyboard and
/// its selection is solid; a press elsewhere then keeps it, in the tint
#[test]
fn a_selection_is_kept_through_the_fields_menu() {
    let theme = theme();
    let mut window = Window::new();
    let short: ShortTextBox = ShortTextBox::starting_with("hello world");
    fn view(short: &ShortTextBox) -> Element<'_, Message> {
        crate::text_menu(
            field(short),
            &crate::text_menu::tests::words(),
            crate::text_menu::tests::style(),
        )
        .history(TextMenuHistory::of(short, Message::Box))
        .into()
    }
    window.send(view(&short), &click(IN_FIELD));
    window.send(view(&short), &[press("a", Code::KeyA, Modifiers::CTRL)]);
    let right = [
        (
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
            IN_FIELD,
        ),
        (
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right)),
            IN_FIELD,
        ),
    ];
    window.send(view(&short), &right);
    let open = window.shot(view(&short), &theme);
    let (accent, tinted) = accent_and_tint(&theme, open.at(Point::new(300.0, 16.0)));
    assert!(
        open.count(accent) > 100,
        "the menu open, the selection is solid"
    );
    window.send(view(&short), &click(Point::new(315.0, 115.0)));
    let kept = window.shot(view(&short), &theme);
    assert!(
        kept.count(tinted) > 100,
        "kept after the menu: {} pixels",
        kept.count(tinted)
    );
}

/// Two fields: the one left keeps its selection, in the tint, while the
/// other has the keyboard and what is typed goes to it alone
#[test]
fn of_two_fields_the_one_left_keeps_its_selection() {
    let theme = theme();
    let mut window = Window::new();
    let (mut first, mut second): (ShortTextBox, ShortTextBox) = (
        ShortTextBox::starting_with("hello world"),
        ShortTextBox::starting_with("other"),
    );
    #[derive(Clone, Debug)]
    enum Two {
        First(Action),
        Second(Action),
    }
    fn view<'a>(first: &'a ShortTextBox, second: &'a ShortTextBox) -> Element<'a, Two> {
        widget::column::with_children(vec![
            short_text_box::text(first, "", Two::First).into(),
            short_text_box::text(second, "", Two::Second).into(),
        ])
        .into()
    }
    fn apply(first: &mut ShortTextBox, second: &mut ShortTextBox, messages: Vec<Two>) {
        for message in messages {
            match message {
                Two::First(action) => first.perform(action),
                Two::Second(action) => second.perform(action),
            };
        }
    }
    let (messages, _) = window.send(view(&first, &second), &click(IN_FIELD));
    apply(&mut first, &mut second, messages);
    let (messages, _) = window.send(
        view(&first, &second),
        &[press("a", Code::KeyA, Modifiers::CTRL)],
    );
    apply(&mut first, &mut second, messages);
    let in_second = Point::new(150.0, 52.0);
    let (messages, _) = window.send(view(&first, &second), &click(in_second));
    apply(&mut first, &mut second, messages);
    let mut typed = press("s", Code::KeyS, Modifiers::empty());
    typed.1 = in_second;
    let (messages, _) = window.send(view(&first, &second), &[typed]);
    apply(&mut first, &mut second, messages);
    assert_eq!((first.value(), &second[..5]), ("hello world", "other"));
    assert_eq!(second.len(), 6, "the typing went to the second");
    let shot = window.shot(view(&first, &second), &theme);
    let (accent, tinted) = accent_and_tint(&theme, shot.at(Point::new(300.0, 16.0)));
    assert!(
        shot.count_in(0..32, tinted) > 100,
        "the first keeps its selection: {} pixels",
        shot.count_in(0..32, tinted)
    );
    assert!(shot.count_in(0..32, accent) < 10);
}

/// The app setting the value while a selection is kept: the selection drawn
/// would be of the old value, so it goes
#[test]
fn the_apps_new_value_lets_a_kept_selection_go() {
    let theme = theme();
    let mut window = Window::new();
    let mut short: ShortTextBox = ShortTextBox::starting_with("hello world");
    window.send(field(&short), &click(IN_FIELD));
    window.send(field(&short), &[press("a", Code::KeyA, Modifiers::CTRL)]);
    window.send(field(&short), &click(BELOW));
    let kept = window.shot(field(&short), &theme);
    let (_, tinted) = accent_and_tint(&theme, kept.at(Point::new(300.0, 16.0)));
    assert!(kept.count(tinted) > 100);
    short.replace("something else", Caret::End);
    window.send(field(&short), &[]);
    let after = window.shot(field(&short), &theme);
    assert!(after.count(tinted) < 10, "{} pixels", after.count(tinted));
}

// ---------------------------------------------------------------------------
// The look: two kinds of text box (rule 25a, Juliet 8 Nov)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Kind {
    Prominent,
    NormalEditor,
    NormalField,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Is {
    Rest,
    Hovered,
    Focused,
    Behind,
    Kept,
}

fn editor_view(text_box: &TextBox, look: Look) -> Element<'_, Message> {
    text_box::wrap(
        text_box::editor(text_box, look, |_| Message::Submit).height(40.0),
        text_box,
        |_| Message::Submit,
    )
}

/// `kind` in the state `is`, drawn in `theme`
fn looked(kind: Kind, is: Is, theme: &Theme) -> Shot {
    let mut window = Window::new();
    let selected: TextBox = TextBox::starting_with(
        "hello world",
        text_box::Caret::At(Cursor {
            position: Position {
                line: 0,
                column: 11,
            },
            selection: Some(Position { line: 0, column: 0 }),
        }),
    );
    let short: ShortTextBox = ShortTextBox::starting_with("hello world");
    let behind = is == Is::Behind;
    let view = || -> Element<'_, Message> {
        match kind {
            Kind::Prominent => editor_view(&selected, Look::prominent().behind(behind)),
            Kind::NormalEditor => editor_view(&selected, Look::normal().behind(behind)),
            Kind::NormalField => field(&short).behind(behind).into(),
        }
    };
    let mut events = Vec::new();
    if matches!(is, Is::Focused | Is::Behind | Is::Kept) {
        events.extend(click(IN_FIELD));
    }
    if is == Is::Kept {
        events.push(press("a", Code::KeyA, Modifiers::CTRL));
        events.extend(click(BELOW));
    }
    window.send(view(), &events);
    let pointer = if is == Is::Hovered { IN_FIELD } else { BELOW };
    window.shot_at(view(), theme, pointer)
}

fn hex(colour: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", colour[0], colour[1], colour[2])
}

/// Each kind in each state, on COSMIC's own dark and light themes, against
/// the 8 Nov sheet's values (its report, `ui-evidence/text-box-look-sheet`),
/// the normal box's focused border being the muted accent the rule settled
/// on in place of the sheet's faded one
#[test]
fn each_kind_of_text_box_in_each_state_is_drawn_as_the_rule_says() {
    let mut wrong = Vec::new();
    for (theme, page, accent, rest, hovered, faded) in [
        (
            Theme::dark(),
            "#1b1b1b",
            "#63d0df",
            "#5c5c5c",
            "#8d8d8d",
            "#60969e",
        ),
        (
            Theme::light(),
            "#ebebeb",
            "#00525a",
            "#b0b0b0",
            "#787878",
            "#588185",
        ),
    ] {
        let dark = theme.cosmic().is_dark;
        assert_eq!(hex(rgb8(theme.cosmic().accent_color().into())), accent);
        let muted =
            household_tones::muted_accent(Srgb::parse(accent).expect("a colour"), dark).to_hex();
        for kind in [Kind::Prominent, Kind::NormalEditor, Kind::NormalField] {
            for is in [Is::Rest, Is::Hovered, Is::Focused, Is::Behind, Is::Kept] {
                let want = match (kind, is) {
                    (Kind::Prominent, Is::Focused) => accent.to_string(),
                    (Kind::Prominent, Is::Behind) => faded.to_string(),
                    (Kind::Prominent, _) => rest.to_string(),
                    (_, Is::Hovered) => hovered.to_string(),
                    (_, Is::Focused) => muted.clone(),
                    (_, Is::Rest | Is::Behind | Is::Kept) => rest.to_string(),
                };
                let shot = looked(kind, is, &theme);
                // The border's outermost pixel, half way down the box
                let edge = hex(shot.at(Point::new(0.0, 16.0)));
                let ground = hex(shot.at(Point::new(300.0, 16.0)));
                if edge != want || ground != page {
                    wrong.push(format!(
                        "{} {kind:?} {is:?}: border {edge}, want {want}; ground {ground}, want {page}",
                        if dark { "dark" } else { "light" }
                    ));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// The width of each kind's border: 4 px prominent, 1 px normal at rest and
/// 2 px focused
#[test]
fn each_kind_of_text_box_has_its_border_width() {
    let theme = Theme::dark();
    let page = [0x1b, 0x1b, 0x1b];
    let width = |shot: &Shot| {
        (0..8)
            .take_while(|x| shot.at(Point::new(*x as f32, 16.0)) != page)
            .count()
    };
    assert_eq!(width(&looked(Kind::Prominent, Is::Rest, &theme)), 4);
    assert_eq!(width(&looked(Kind::Prominent, Is::Focused, &theme)), 4);
    assert_eq!(width(&looked(Kind::NormalField, Is::Rest, &theme)), 1);
    assert_eq!(width(&looked(Kind::NormalField, Is::Focused, &theme)), 2);
    assert_eq!(width(&looked(Kind::NormalEditor, Is::Focused, &theme)), 2);
}

/// A prominent box in a window behind others fades its own window's accent,
/// not the accent of the theme it is drawn in (the window in front's)
#[test]
fn a_prominent_box_behind_fades_its_own_windows_accent() {
    let theme = Theme::dark();
    let own = Color::from_rgb8(0xd0, 0x63, 0x63);
    let look = Look::prominent().behind(true).accent(own);
    let border = look.border(&theme, crate::look::State::Focused).color;
    let page = household_tones::Srgb::hex(0x1b1b1b);
    let rgba = |c: Color| household_tones::Srgba::new(c.r, c.g, c.b, c.a);
    let rest = household_tones::DARK_REST_BORDER.with_alpha(1.0);
    let want = household_tones::faded_accent(rgba(own), rest, page);
    assert_eq!(hex(rgb8(border)), want.to_hex());
    let front = Look::prominent().accent(own);
    assert_eq!(
        hex(rgb8(
            front.border(&theme, crate::look::State::Focused).color
        )),
        "#d06363",
        "in front, its own accent"
    );
}

/// Every step a field's edit starts can carry the app's part (a search
/// field's match), handed back by its Undo and Redo; typing that joins a
/// step keeps the part the step began with
#[test]
fn a_fields_own_steps_carry_the_apps_part() {
    let mut short: ShortTextBox<usize> = ShortTextBox::starting_with("rule");
    // Select all and type over it, the reader at match 2: one step
    short.perform_with(
        Action::Edit(Edit {
            value: "x".into(),
            before: span(0, 4),
            after: span(1, 1),
            by: By::Typed { space: false },
            hidden: false,
        }),
        2,
    );
    short.perform_with(
        Action::Edit(Edit {
            value: "xy".into(),
            before: span(1, 1),
            after: span(2, 2),
            by: By::Typed { space: false },
            hidden: false,
        }),
        0,
    );
    assert_eq!(short.undo_part(), Some(&2), "the part the step began with");
    assert_eq!(short.perform(Action::Undo), Some(Turn::Undone(2)));
    assert_eq!(short.value(), "rule");
    assert_eq!(short.perform(Action::Redo), Some(Turn::Redone(2)));
}

/// Esc closing a field's menu is the menu's: the field keeps its selection,
/// and a press elsewhere then keeps it too, drawn in the tint
#[test]
fn esc_closing_a_fields_menu_keeps_its_selection() {
    let theme = theme();
    let mut window = Window::new();
    let short: ShortTextBox = ShortTextBox::starting_with("hello world");
    fn view(short: &ShortTextBox) -> Element<'_, Message> {
        crate::text_menu(
            field(short),
            &crate::text_menu::tests::words(),
            crate::text_menu::tests::style(),
        )
        .history(TextMenuHistory::of(short, Message::Box))
        .into()
    }
    window.send(view(&short), &click(IN_FIELD));
    window.send(view(&short), &[press("a", Code::KeyA, Modifiers::CTRL)]);
    window.send(
        view(&short),
        &[
            (
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
                IN_FIELD,
            ),
            (
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right)),
                IN_FIELD,
            ),
        ],
    );
    let (_, statuses) = window.send(
        view(&short),
        &[named(keyboard::key::Named::Escape, Code::Escape)],
    );
    assert_eq!(statuses, [Status::Captured], "the menu's Esc");
    window.send(view(&short), &click(Point::new(315.0, 115.0)));
    let kept = window.shot(view(&short), &theme);
    let (_, tinted) = accent_and_tint(&theme, kept.at(Point::new(300.0, 16.0)));
    assert!(
        kept.count(tinted) > 100,
        "kept after Esc: {} pixels",
        kept.count(tinted)
    );
}
