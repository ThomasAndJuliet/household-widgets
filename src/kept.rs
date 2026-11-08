// SPDX-License-Identifier: GPL-3.0-only

//! A selection kept while its box does not have the focus, drawn dimmed, as
//! GTK draws the selection of a box that is not the focus: what a menu's Cut
//! or Copy acts on (the Edit menu open, the box's own menu) is always seen.
//!
//! Neither iced's text editor nor libcosmic's text input draws a selection
//! without the focus: each draws it only when its own state says it is
//! focused. So a box with a kept selection is drawn from a second widget
//! tree, the box's own state as it is when focused, while [`drawn`] tells
//! the box's colours to give the kept look ([`kept_selection_colours`]).
//! Nothing of libcosmic changes; the tree is the box wrapper's own, and
//! only drawing reads it.

use std::cell::Cell;

use cosmic::Theme;
use cosmic::iced::advanced::widget::{Tree, tree};
use cosmic::iced::{Background, Color};
use household_tones::{Srgb, Srgba, on_tint, over, tint};

thread_local! {
    /// While a kept selection is drawn: whether the pointer is over the box
    static DRAWING: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Whether a kept selection is being drawn, and if so whether the pointer
/// is over its box (a text input's look follows the hover)
pub(crate) fn drawing() -> Option<bool> {
    DRAWING.with(Cell::get)
}

/// `draw`, with the box's colours as for a kept selection
pub(crate) fn drawn<R>(hovered: bool, draw: impl FnOnce() -> R) -> R {
    let was = DRAWING.with(|drawing| drawing.replace(Some(hovered)));
    let drawn = draw();
    DRAWING.with(|drawing| drawing.set(was));
    drawn
}

/// The fill and the text colour of a selection kept while its box does not
/// have the focus: the household's selection tint (rule 6, "What is
/// highlighted, and how": the theme's accent at 32 % over `beneath`, the
/// box's background) in place of the solid accent of a box being typed
/// into, and `text`, the box's own text colour, moved only as far as the
/// fill rule needs for reading on the tint (`household_tones::on_tint`, a
/// grey going to `text` itself). No new colour: the accent, and the tint
/// the household already marks selected content with.
pub fn kept_selection_colours(theme: &Theme, text: Color, beneath: Color) -> (Color, Color) {
    let accent = theme.cosmic().accent_color();
    let accent = Srgb::new(accent.red, accent.green, accent.blue);
    let (text, beneath) = (srgb(text), srgb(beneath));
    let fill = tint(accent, beneath);
    (colour(fill), colour(on_tint(text, accent, beneath, text)))
}

/// `background` as one colour, laid over `under`
pub(crate) fn flat(background: Background, under: Color) -> Color {
    let Background::Color(top) = background else {
        return under;
    };
    let rgba = |c: Color| Srgba::new(c.r, c.g, c.b, c.a);
    let flat = over(rgba(top), rgba(Color { a: 1.0, ..under }));
    Color::from_rgb(flat.r, flat.g, flat.b)
}

fn srgb(c: Color) -> Srgb {
    Srgb::new(c.r, c.g, c.b)
}

fn colour(c: Srgb) -> Color {
    Color::from_rgb(c.r, c.g, c.b)
}

/// The first widget state of type `T` in `tree`: its own or below
pub(crate) fn find<T: 'static>(tree: &Tree) -> Option<&T> {
    if tree.tag == tree::Tag::of::<T>()
        && let tree::State::Some(state) = &tree.state
    {
        return state.downcast_ref();
    }
    tree.children.iter().find_map(find)
}

/// [`find`], to change it
pub(crate) fn find_mut<T: 'static>(tree: &mut Tree) -> Option<&mut T> {
    if tree.tag == tree::Tag::of::<T>() {
        return match &mut tree.state {
            tree::State::Some(state) => state.downcast_mut(),
            tree::State::None => None,
        };
    }
    tree.children.iter_mut().find_map(find_mut)
}
