// SPDX-License-Identifier: GPL-3.0-only

//! A text box's history and keys: "This obviously includes the usual
//! functionality of such a box which includes ctrl + z" (Juliet, 6 Nov).

use std::sync::Arc;

use cosmic::Element;
use cosmic::iced::advanced::clipboard;
use cosmic::iced::advanced::renderer::Headless;
use cosmic::iced::keyboard::key::{Code, Named, NativeCode, Physical};
use cosmic::iced::keyboard::{self, Key, Modifiers};
use cosmic::iced::runtime::user_interface::{Cache, UserInterface};
use cosmic::iced::{Event, Font, Pixels, Point, Size, mouse};
use cosmic::widget::text_editor::{self, Binding, Edit, KeyPress, Motion, Position, Status};

use crate::Undoable;
use crate::text_box::{Action, Caret, STEPS, TextBox, Turn, history_key, key_binding, shift_click};

fn typed<P: Clone>(text_box: &mut TextBox<P>, text: &str) {
    for c in text.chars() {
        let edit = if c == '\n' {
            Edit::Enter
        } else {
            Edit::Insert(c)
        };
        text_box.perform(text_editor::Action::Edit(edit));
    }
}

fn edit<P: Clone>(text_box: &mut TextBox<P>, edit: Edit) {
    text_box.perform(text_editor::Action::Edit(edit));
}

fn paste<P: Clone>(text_box: &mut TextBox<P>, text: &str) {
    edit(text_box, Edit::Paste(Arc::new(text.to_string())));
}

/// Undo pressed until nothing is left: the text after each
fn undone<P: Clone>(text_box: &mut TextBox<P>) -> Vec<String> {
    let mut texts = Vec::new();
    while text_box.can_undo() {
        text_box.perform(Action::Undo);
        texts.push(text_box.text());
    }
    texts
}

fn at(line: usize, column: usize) -> Position {
    Position { line, column }
}

#[test]
fn a_run_of_typing_is_one_step_and_a_space_after_a_word_starts_the_next() {
    let mut text_box: TextBox = TextBox::new();
    assert!(!text_box.can_undo() && !text_box.can_redo());
    typed(&mut text_box, "hello  world\nagain");
    assert_eq!(undone(&mut text_box), ["hello  world", "hello", ""]);
    for want in ["hello", "hello  world", "hello  world\nagain"] {
        text_box.perform(Action::Redo);
        assert_eq!(text_box.text(), want);
    }
    assert!(!text_box.can_redo());
}

#[test]
fn moving_the_caret_or_changing_between_typing_and_deleting_ends_a_step() {
    let mut text_box: TextBox = TextBox::new();
    typed(&mut text_box, "abd");
    text_box.perform(text_editor::Action::Move(Motion::Left));
    typed(&mut text_box, "c");
    edit(&mut text_box, Edit::Backspace);
    edit(&mut text_box, Edit::Backspace);
    typed(&mut text_box, "xy");
    assert_eq!(text_box.text(), "axyd");
    assert_eq!(undone(&mut text_box), ["ad", "abcd", "abd", ""]);
}

#[test]
fn undo_puts_back_the_caret_and_the_selection() {
    let mut text_box: TextBox = TextBox::new();
    typed(&mut text_box, "abd");
    text_box.perform(text_editor::Action::Move(Motion::Left));
    typed(&mut text_box, "c");
    text_box.perform(Action::Undo);
    assert_eq!(
        text_box.cursor().position,
        at(0, 2),
        "where the caret was before the step"
    );

    let mut text_box: TextBox = TextBox::starting_with("hello world", Caret::Start);
    text_box.perform(text_editor::Action::SelectAll);
    let selected = text_box.cursor();
    edit(&mut text_box, Edit::Backspace);
    assert_eq!(text_box.text(), "");
    text_box.perform(Action::Undo);
    assert_eq!(text_box.text(), "hello world");
    assert_eq!(text_box.cursor(), selected, "the selection is back");
    assert_eq!(text_box.selection().as_deref(), Some("hello world"));
}

#[test]
fn a_paste_a_cut_and_deleting_a_selection_are_a_step_each() {
    let mut text_box: TextBox = TextBox::new();
    typed(&mut text_box, "ab");
    paste(&mut text_box, "XY");
    typed(&mut text_box, "c");
    // A cut is iced's Delete of the selection
    text_box.perform(text_editor::Action::Select(Motion::Left));
    edit(&mut text_box, Edit::Delete);
    text_box.perform(text_editor::Action::Select(Motion::Left));
    edit(&mut text_box, Edit::Backspace);
    assert_eq!(text_box.text(), "abX");
    assert_eq!(undone(&mut text_box), ["abXY", "abXYc", "abXY", "ab", ""]);
}

#[test]
fn typing_over_a_selection_is_one_step_with_the_typing_after_it() {
    let mut text_box: TextBox = TextBox::starting_with("old", Caret::Start);
    text_box.perform(text_editor::Action::SelectAll);
    typed(&mut text_box, "new");
    assert_eq!(undone(&mut text_box), ["old"]);
}

#[test]
fn a_key_that_changes_nothing_is_no_step_and_joins_none() {
    let mut text_box: TextBox = TextBox::new();
    edit(&mut text_box, Edit::Backspace);
    assert!(!text_box.can_undo());
    typed(&mut text_box, "a");
    text_box.perform(text_editor::Action::Move(Motion::DocumentStart));
    edit(&mut text_box, Edit::Backspace);
    edit(&mut text_box, Edit::Delete);
    assert_eq!(undone(&mut text_box), ["a", ""]);
}

#[test]
fn a_replacement_is_one_step_with_the_caret_at_its_end() {
    let mut text_box: TextBox = TextBox::starting_with("draft", Caret::Start);
    assert!(!text_box.can_undo(), "a new box has nothing to undo");
    text_box.replace("/help ", Caret::End);
    assert_eq!(text_box.cursor().position, at(0, 6));
    typed(&mut text_box, "me");
    assert_eq!(undone(&mut text_box), ["/help ", "draft"]);
    // The same text again: no step, the caret still put at the end
    let mut text_box: TextBox = TextBox::starting_with("same", Caret::Start);
    text_box.replace("same", Caret::End);
    assert!(!text_box.can_undo());
    assert_eq!(text_box.cursor().position, at(0, 4));
}

#[test]
fn typing_after_undo_drops_the_steps_redo_had() {
    let mut text_box: TextBox = TextBox::new();
    typed(&mut text_box, "one two");
    text_box.perform(Action::Undo);
    assert!(text_box.can_redo());
    typed(&mut text_box, "!");
    assert!(!text_box.can_redo());
    assert_eq!(text_box.text(), "one!");
    text_box.perform(Action::Redo);
    assert_eq!(
        text_box.text(),
        "one!",
        "Redo with nothing to redo does nothing"
    );
}

#[test]
fn the_history_keeps_the_last_steps() {
    let mut text_box: TextBox = TextBox::new();
    for _ in 0..STEPS + 20 {
        paste(&mut text_box, "x");
    }
    let undone = undone(&mut text_box);
    assert_eq!(undone.len(), STEPS);
    assert_eq!(undone.last().map(String::len), Some(20));
}

#[test]
fn a_word_is_deleted_as_one_step_or_the_selection_if_there_is_one() {
    let mut text_box: TextBox = TextBox::starting_with("hello big world", Caret::Start);
    text_box.perform(text_editor::Action::Move(Motion::DocumentEnd));
    text_box.perform(Action::DeleteWordBefore);
    assert_eq!(text_box.text(), "hello big ");
    text_box.perform(text_editor::Action::Move(Motion::DocumentStart));
    text_box.perform(Action::DeleteWordAfter);
    assert_eq!(text_box.text().trim_start(), "big ");
    text_box.perform(text_editor::Action::SelectAll);
    text_box.perform(Action::DeleteWordBefore);
    assert_eq!(text_box.text(), "");
    assert_eq!(undone(&mut text_box).len(), 3);
    assert_eq!(text_box.text(), "hello big world");
}

fn press(key: Key, physical: Physical, modifiers: Modifiers, focused: bool) -> KeyPress {
    let text = match &key {
        Key::Character(c) if !modifiers.control() => Some(c.clone()),
        _ => None,
    };
    KeyPress {
        modified_key: key.clone(),
        key,
        physical_key: physical,
        modifiers,
        text,
        status: if focused {
            Status::Focused { is_hovered: false }
        } else {
            Status::Active
        },
    }
}

fn letter(c: &str, code: Code, modifiers: Modifiers) -> KeyPress {
    press(
        Key::Character(c.into()),
        Physical::Code(code),
        modifiers,
        true,
    )
}

fn named(named: Named, modifiers: Modifiers) -> KeyPress {
    press(
        Key::Named(named),
        Physical::Unidentified(NativeCode::Unidentified),
        modifiers,
        true,
    )
}

fn bound(press: KeyPress) -> Option<Action> {
    match key_binding(press, |action| action)? {
        Binding::Custom(action) => Some(action),
        Binding::Move(motion) => Some(Action::Editor(text_editor::Action::Move(motion))),
        Binding::Select(motion) => Some(Action::Editor(text_editor::Action::Select(motion))),
        Binding::Insert(c) => Some(Action::Editor(text_editor::Action::Edit(Edit::Insert(c)))),
        Binding::Backspace => Some(Action::Editor(text_editor::Action::Edit(Edit::Backspace))),
        Binding::SelectAll => Some(Action::Editor(text_editor::Action::SelectAll)),
        _ => None,
    }
}

#[test]
fn the_standard_keys_include_undo_redo_and_deleting_a_word() {
    let ctrl = Modifiers::CTRL;
    let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
    assert_eq!(bound(letter("z", Code::KeyZ, ctrl)), Some(Action::Undo));
    assert_eq!(
        bound(letter("Z", Code::KeyZ, ctrl_shift)),
        Some(Action::Redo)
    );
    assert_eq!(
        bound(letter("z", Code::KeyZ, ctrl_shift)),
        Some(Action::Redo)
    );
    assert_eq!(bound(letter("y", Code::KeyY, ctrl)), Some(Action::Redo));
    // Another layout's letter on the Z key
    assert_eq!(bound(letter("я", Code::KeyZ, ctrl)), Some(Action::Undo));
    assert_eq!(
        bound(named(Named::Backspace, ctrl)),
        Some(Action::DeleteWordBefore)
    );
    assert_eq!(
        bound(named(Named::Delete, ctrl)),
        Some(Action::DeleteWordAfter)
    );
    // iced's own, as before
    let editor = |action| Some(Action::Editor(action));
    assert_eq!(
        bound(letter("z", Code::KeyZ, Modifiers::empty())),
        editor(text_editor::Action::Edit(Edit::Insert('z')))
    );
    assert_eq!(
        bound(named(Named::Backspace, Modifiers::empty())),
        editor(text_editor::Action::Edit(Edit::Backspace))
    );
    assert_eq!(
        bound(letter("a", Code::KeyA, ctrl)),
        editor(text_editor::Action::SelectAll)
    );
    assert_eq!(
        bound(named(Named::ArrowLeft, ctrl)),
        editor(text_editor::Action::Move(Motion::WordLeft))
    );
    assert_eq!(
        bound(named(Named::ArrowRight, ctrl_shift)),
        editor(text_editor::Action::Select(Motion::WordRight))
    );
    assert_eq!(
        bound(named(Named::Home, ctrl_shift)),
        editor(text_editor::Action::Select(Motion::DocumentStart))
    );
    assert_eq!(
        bound(named(Named::End, Modifiers::SHIFT)),
        editor(text_editor::Action::Select(Motion::End))
    );
}

/// The key's Latin letter decides, not the key's place on the keyboard
/// (rule 21c): QWERTZ has Z where QWERTY has Y, Dvorak has it elsewhere
/// still, and a Cyrillic letter is read by the key it is on
#[test]
fn undo_and_redo_follow_the_latin_letter_on_any_layout() {
    let ctrl = Modifiers::CTRL;
    let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
    // QWERTZ: Z is on the physical Y key, and Y on the physical Z key
    assert_eq!(bound(letter("z", Code::KeyY, ctrl)), Some(Action::Undo));
    assert_eq!(
        bound(letter("Z", Code::KeyY, ctrl_shift)),
        Some(Action::Redo)
    );
    assert_eq!(bound(letter("y", Code::KeyZ, ctrl)), Some(Action::Redo));
    // Dvorak: Z is on the physical Semicolon key
    assert_eq!(
        bound(letter("z", Code::Semicolon, ctrl)),
        Some(Action::Undo)
    );
    // A Cyrillic letter on the keys of Z and Y: read by the key
    assert_eq!(bound(letter("я", Code::KeyZ, ctrl)), Some(Action::Undo));
    assert_eq!(bound(letter("н", Code::KeyY, ctrl)), Some(Action::Redo));
    // ... and on a key that is neither, it is not undo
    assert_ne!(bound(letter("я", Code::KeyQ, ctrl)), Some(Action::Undo));
}

/// Keys that are not Ctrl+Z: with Alt (AltGr is Ctrl+Alt on some systems),
/// with the logo key, or Ctrl+Shift+Y; and Ctrl+Backspace with Shift or Alt
/// is not a word's deletion
#[test]
fn near_misses_of_the_history_and_word_keys_are_not_them() {
    let ctrl_alt = Modifiers::CTRL | Modifiers::ALT;
    let ctrl_logo = Modifiers::CTRL | Modifiers::LOGO;
    let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
    for modifiers in [ctrl_alt, ctrl_logo, Modifiers::LOGO, Modifiers::ALT] {
        for (c, code) in [("z", Code::KeyZ), ("Z", Code::KeyZ), ("y", Code::KeyY)] {
            let got = bound(letter(c, code, modifiers));
            assert!(
                !matches!(got, Some(Action::Undo | Action::Redo)),
                "{c} with {modifiers:?} is {got:?}"
            );
        }
    }
    let got = bound(letter("y", Code::KeyY, ctrl_shift));
    assert!(
        !matches!(got, Some(Action::Undo | Action::Redo)),
        "Ctrl+Shift+Y is {got:?}"
    );
    for modifiers in [ctrl_shift, ctrl_alt] {
        for key in [Named::Backspace, Named::Delete] {
            let got = bound(named(key, modifiers));
            assert!(
                !matches!(
                    got,
                    Some(Action::DeleteWordBefore | Action::DeleteWordAfter)
                ),
                "{key:?} with {modifiers:?} is {got:?}"
            );
        }
    }
}

/// Deleting a word where there is none to delete (before the start, after
/// the end) changes nothing and is no step, so that Ctrl+Z still goes back
/// to the typing before it
#[test]
fn deleting_a_word_past_either_end_is_no_step() {
    let mut text_box: TextBox = TextBox::starting_with("abc", Caret::Start);
    text_box.perform(Action::DeleteWordBefore);
    assert_eq!(text_box.text(), "abc");
    assert!(!text_box.can_undo(), "nothing before the start was a step");
    text_box.perform(text_editor::Action::Move(Motion::DocumentEnd));
    text_box.perform(Action::DeleteWordAfter);
    assert_eq!(text_box.text(), "abc");
    assert!(!text_box.can_undo(), "nothing after the end was a step");
    // Typing before and after them is still one step
    let mut typed_box: TextBox = TextBox::new();
    typed(&mut typed_box, "abc");
    typed_box.perform(text_editor::Action::Move(Motion::DocumentStart));
    typed_box.perform(Action::DeleteWordBefore);
    typed_box.perform(text_editor::Action::Move(Motion::DocumentEnd));
    typed_box.perform(Action::DeleteWordAfter);
    assert_eq!(undone(&mut typed_box), [""]);
}

/// A word deleted at the start of a line takes the line break and goes
/// back in one step
#[test]
fn deleting_a_word_at_a_line_start_is_one_step() {
    let mut text_box: TextBox = TextBox::starting_with("first line\nsecond", Caret::Start);
    text_box.perform(text_editor::Action::Move(Motion::DocumentEnd));
    text_box.perform(text_editor::Action::Move(Motion::Home));
    assert_eq!(text_box.cursor().position, at(1, 0));
    text_box.perform(Action::DeleteWordBefore);
    let text = text_box.text();
    assert!(
        !text.contains('\n') && text.starts_with("first") && text.ends_with("second"),
        "{text:?}"
    );
    assert_eq!(undone(&mut text_box), ["first line\nsecond"]);
}

#[test]
fn an_unfocused_box_takes_no_key() {
    let unfocused = press(
        Key::Character("z".into()),
        Physical::Code(Code::KeyZ),
        Modifiers::CTRL,
        false,
    );
    assert!(
        key_binding(unfocused, |action| action).is_none(),
        "the app's Ctrl+Z, or the terminal's"
    );
}

#[derive(Default)]
struct Board;

impl clipboard::Clipboard for Board {
    fn read(&self, _: clipboard::Kind) -> Option<String> {
        None
    }

    fn write(&mut self, _: clipboard::Kind, _: String) {}
}

/// `events` through a focused-or-not box holding `text_box`, in a window
/// 300 x 60; what it published
fn through(text_box: &TextBox, events: &[Event], at: Point) -> Vec<Action> {
    let mut renderer = cosmic::iced::futures::executor::block_on(
        <cosmic::Renderer as Headless>::new(Font::default(), Pixels(14.0), Some("tiny-skia")),
    )
    .expect("a headless renderer");
    let element: Element<'_, Action> = shift_click(
        text_editor::text_editor(text_box)
            .on_action(Action::from)
            .key_binding(|press| key_binding(press, |action| action)),
        Action::Extend,
    );
    let mut ui = UserInterface::build(
        element,
        Size::new(300.0, 60.0),
        Cache::default(),
        &mut renderer,
    );
    let mut messages = Vec::new();
    let _ = ui.update(
        events,
        mouse::Cursor::Available(at),
        &mut renderer,
        &mut Board,
        &mut messages,
    );
    messages
}

#[test]
fn shift_click_extends_the_selection_from_where_it_was() {
    let mut text_box: TextBox = TextBox::starting_with("hello world", Caret::Start);
    let left = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    let release = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    let shift = |modifiers| Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers));
    // A plain click places the caret, past "hello"
    let far = Point::new(200.0, 10.0);
    for action in through(&text_box, &[left.clone(), release.clone()], far) {
        text_box.perform(action);
    }
    assert!(text_box.selection().is_none());
    let end = text_box.cursor().position;
    assert_eq!(end, at(0, 11));
    // Shift held, a click near the start: from the caret to there
    let near = Point::new(8.0, 10.0);
    let said = through(
        &text_box,
        &[shift(Modifiers::SHIFT), left.clone(), release.clone()],
        near,
    );
    assert!(
        matches!(
            said.as_slice(),
            [
                Action::Editor(text_editor::Action::Click(_)),
                Action::Extend
            ]
        ),
        "{said:?}"
    );
    for action in said {
        text_box.perform(action);
    }
    assert_eq!(
        text_box.cursor().selection,
        Some(end),
        "anchored where the caret was"
    );
    assert!(
        text_box
            .selection()
            .is_some_and(|selected| selected.ends_with("world"))
    );
    // Shift let go: a click is a click again
    let said = through(
        &text_box,
        &[shift(Modifiers::SHIFT), shift(Modifiers::empty()), left],
        far,
    );
    assert!(
        matches!(
            said.as_slice(),
            [Action::Editor(text_editor::Action::Click(_))]
        ),
        "{said:?}"
    );
}

/// Shift+click again quick on the same spot, where iced sees a double
/// click (it selects a word, the caret not moved): still the selection
/// extended, from where it began to the caret
#[test]
fn shift_click_quick_after_a_click_still_extends() {
    let mut text_box: TextBox = TextBox::starting_with("hello world", Caret::End);
    let left = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    let release = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
    let shift = Event::Keyboard(keyboard::Event::ModifiersChanged(Modifiers::SHIFT));
    let near = Point::new(8.0, 10.0);
    let said = through(
        &text_box,
        &[shift, left.clone(), release.clone(), left, release],
        near,
    );
    assert!(
        matches!(
            said.as_slice(),
            [
                Action::Editor(text_editor::Action::Click(_)),
                Action::Extend,
                Action::Editor(text_editor::Action::SelectWord),
                Action::Extend
            ]
        ),
        "{said:?}"
    );
    for action in said {
        text_box.perform(action);
    }
    // Not the word "hello": from the end, where the caret was, to the click
    assert_eq!(text_box.cursor().selection, Some(at(0, 11)));
    assert!(
        text_box
            .selection()
            .is_some_and(|selected| selected.ends_with("world") && selected != "hello"),
        "{:?}",
        text_box.selection()
    );
}

/// A Shift+press on a box that cannot act (no `on_action`) sends nothing;
/// and an Extend that does not follow its click does nothing, whatever
/// came between
#[test]
fn an_extend_acts_only_on_the_click_just_done() {
    let text_box: TextBox = TextBox::starting_with("hello world", Caret::Start);
    let mut renderer = cosmic::iced::futures::executor::block_on(
        <cosmic::Renderer as Headless>::new(Font::default(), Pixels(14.0), Some("tiny-skia")),
    )
    .expect("a headless renderer");
    let element: Element<'_, Action> =
        shift_click(text_editor::text_editor(&text_box), Action::Extend);
    let mut ui = UserInterface::build(
        element,
        Size::new(300.0, 60.0),
        Cache::default(),
        &mut renderer,
    );
    let mut said = Vec::new();
    let events = [
        Event::Keyboard(keyboard::Event::ModifiersChanged(Modifiers::SHIFT)),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
    ];
    let _ = ui.update(
        &events,
        mouse::Cursor::Available(Point::new(8.0, 10.0)),
        &mut renderer,
        &mut Board,
        &mut said,
    );
    assert!(said.is_empty(), "a read-only box: {said:?}");
    drop(ui);

    let mut text_box: TextBox = TextBox::starting_with("hello world", Caret::End);
    text_box.perform(Action::Extend);
    assert!(text_box.cursor().selection.is_none(), "no click before it");
    text_box.perform(text_editor::Action::Click(Point::new(8.0, 10.0)));
    text_box.replace("other", Caret::End);
    text_box.perform(Action::Extend);
    assert!(text_box.selection().is_none(), "a replacement between");
}

/// A word or line selected by a double or triple click is put back whole by
/// Undo, not only the point clicked
#[test]
fn undo_puts_back_a_word_or_line_selection() {
    for (select, want) in [
        (text_editor::Action::SelectWord, "brown"),
        (text_editor::Action::SelectLine, "the brown fox"),
    ] {
        let mut text_box: TextBox = TextBox::starting_with("the brown fox\nnext", Caret::Start);
        for _ in 0..6 {
            text_box.perform(text_editor::Action::Move(Motion::Right));
        }
        text_box.perform(select);
        assert_eq!(
            text_box.selection().as_deref().map(str::trim_end),
            Some(want)
        );
        edit(&mut text_box, Edit::Backspace);
        text_box.perform(Action::Undo);
        assert_eq!(
            text_box.selection().as_deref().map(str::trim_end),
            Some(want)
        );
    }
}

/// A change made in several goes, as saves from another editor: one step
/// back to before the first
#[test]
fn an_amended_change_is_one_step() {
    let mut text_box: TextBox = TextBox::starting_with("draft", Caret::End);
    text_box.replace("draft, saved once", Caret::End);
    text_box.amend("draft, saved twice", Caret::End);
    text_box.amend("draft, saved three times", Caret::End);
    assert_eq!(undone(&mut text_box), ["draft"]);
    text_box.perform(Action::Redo);
    assert_eq!(text_box.text(), "draft, saved three times");
}

/// Where the app puts the caret is where it goes, inside the text
#[test]
fn the_caret_goes_where_the_app_says() {
    let start: TextBox = TextBox::starting_with("one\ntwo", Caret::Start);
    assert_eq!(start.cursor().position, at(0, 0));
    let end: TextBox = TextBox::starting_with("one\ntwo", Caret::End);
    assert_eq!(end.cursor().position, at(1, 3));
    let cursor = text_editor::Cursor {
        position: at(0, 2),
        selection: Some(at(1, 1)),
    };
    let mut text_box: TextBox = TextBox::new();
    text_box.replace("one\ntwo", Caret::At(cursor));
    assert_eq!(text_box.cursor(), cursor);
    // Past the text: at its end, and inside a character
    let far = text_editor::Cursor {
        position: at(9, 9),
        selection: None,
    };
    assert_eq!(
        TextBox::<()>::starting_with("é", Caret::At(far))
            .cursor()
            .position,
        at(0, 2)
    );
    let inside = text_editor::Cursor {
        position: at(0, 1),
        selection: None,
    };
    assert_eq!(
        TextBox::<()>::starting_with("é", Caret::At(inside))
            .cursor()
            .position,
        at(0, 0)
    );
}

/// The undo keys alone, for keys that reach a box some other way
#[test]
fn the_history_keys_are_shared() {
    let z = Key::Character("z".into());
    assert_eq!(
        history_key(&z, Physical::Code(Code::KeyZ), Modifiers::CTRL),
        Some(Action::Undo)
    );
    assert_eq!(
        history_key(
            &z,
            Physical::Code(Code::KeyZ),
            Modifiers::CTRL | Modifiers::SHIFT
        ),
        Some(Action::Redo)
    );
    assert_eq!(
        history_key(&z, Physical::Code(Code::KeyZ), Modifiers::empty()),
        None
    );
}

/// A step with the app's own part (a card stashed with the text): Undo
/// and Redo hand the part back for the app to undo or redo its side; a
/// part that can no longer be undone ends the history there
#[test]
fn a_step_with_the_apps_part_hands_it_back() {
    let mut text_box: TextBox<&str> = TextBox::starting_with("a prompt", Caret::End);
    typed(&mut text_box, "!");
    text_box.replace_with("", Caret::Start, "stashed");
    // Always a step, the text changed or not
    text_box.replace_with("", Caret::Start, "nothing to the text");
    assert_eq!(text_box.undo_part(), Some(&"nothing to the text"));
    assert_eq!(
        text_box.perform(Action::Undo),
        Some(Turn::Undone("nothing to the text"))
    );
    assert_eq!(
        text_box.perform(Action::Undo),
        Some(Turn::Undone("stashed"))
    );
    assert_eq!(text_box.text(), "a prompt!");
    assert_eq!(text_box.redo_part(), Some(&"stashed"));
    assert_eq!(
        text_box.perform(Action::Redo),
        Some(Turn::Redone("stashed"))
    );
    assert_eq!(text_box.text(), "");
    // A step of the text alone hands nothing back
    text_box.perform(Action::Undo);
    assert_eq!(text_box.perform(Action::Undo), None);
    assert_eq!(text_box.text(), "a prompt");
}

/// Parts that can no longer be undone or redone: the history ends at the
/// nearest step with one, either way, and the steps of the text alone
/// between it and now are kept
#[test]
fn forgetting_the_parts_keeps_the_typing_nearer_now() {
    let mut text_box: TextBox<&str> = TextBox::new();
    typed(&mut text_box, "before");
    text_box.replace_with("", Caret::Start, "stashed");
    typed(&mut text_box, "one two");
    // Undone back to the stash; a step of typing further on to redo
    text_box.replace_with("x", Caret::End, "later");
    text_box.perform(Action::Undo);
    assert_eq!(text_box.text(), "one two");
    text_box.forget_parts();
    // Back through the typing after the stash, not past it
    assert_eq!(undone(&mut text_box), ["one", ""]);
    assert!(!text_box.can_redo() || text_box.redo_part().is_none());
    // Forward again through the typing; the later step with a part is gone
    while text_box.can_redo() {
        assert_eq!(text_box.perform(Action::Redo), None);
    }
    assert_eq!(text_box.text(), "one two");
}

/// The standard keys are one list, which the key binding goes by
#[test]
fn the_standard_keys_are_one_list() {
    let keys = crate::text_box::standard_keys();
    let actions: Vec<Action> = keys.iter().map(|(_, action)| action.clone()).collect();
    assert_eq!(
        actions,
        [
            Action::Undo,
            Action::Redo,
            Action::Redo,
            Action::DeleteWordBefore,
            Action::DeleteWordAfter
        ]
    );
    for (bind, action) in keys {
        let mut modifiers = Modifiers::empty();
        for modifier in &bind.modifiers {
            modifiers |= match modifier {
                cosmic::widget::menu::key_bind::Modifier::Ctrl => Modifiers::CTRL,
                cosmic::widget::menu::key_bind::Modifier::Shift => Modifiers::SHIFT,
                _ => Modifiers::empty(),
            };
        }
        let physical = Physical::Unidentified(NativeCode::Unidentified);
        assert_eq!(
            bound(press(bind.key.clone(), physical, modifiers, true)),
            Some(action),
            "{bind:?}"
        );
    }
}

/// Losing the focus (a menu opened) does not lose the selection: iced's
/// ClearSelection is kept from it, for the menu's Cut and Copy
#[test]
fn losing_the_focus_keeps_the_selection() {
    let mut text_box: TextBox = TextBox::starting_with("hello world", Caret::End);
    text_box.perform(text_editor::Action::SelectAll);
    text_box.perform(text_editor::Action::ClearSelection);
    assert_eq!(text_box.selection().as_deref(), Some("hello world"));
}
