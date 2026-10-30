// SPDX-License-Identifier: GPL-3.0-only

//! libcosmic's switch in the muted accent: "yes use the muted accent for
//! them all" (Juliet, 30 Oct; household rule 6). Only the "on" fill is
//! ours, through libcosmic's own toggler class: hover and disabled are
//! derived from that fill as from the accent, and the rest is libcosmic's.

use cosmic::iced::Color;
use cosmic::widget::list::{self, ListButton};
use cosmic::widget::settings::item::Item;
use cosmic::widget::{self, Toggler};
use cosmic::{Element, Theme, theme};
use household_tones::{Srgb, muted_accent, on_muted};

/// The muted accent of `theme`'s accent (household rule 6), and the text and
/// icon colour on it. The accent's alpha is kept.
pub fn muted_colours(theme: &Theme) -> (Color, Color) {
    let c = theme.cosmic();
    let accent = c.accent_color();
    let muted = muted_accent(Srgb::new(accent.red, accent.green, accent.blue), c.is_dark);
    let on = on_muted(muted);
    let colour = |c: Srgb| Color {
        r: c.r,
        g: c.g,
        b: c.b,
        a: accent.alpha,
    };
    (colour(muted), colour(on))
}

/// libcosmic's `widget::toggler` in the muted accent, for a switch with
/// options: `toggler(on).label(label).spacing(8).on_toggle(message)`. Without
/// `on_toggle` it is disabled.
pub fn toggler<'a, M>(on: bool) -> Toggler<'a, M> {
    widget::toggler(on).class(theme::Toggler::accent(|t: &Theme| muted_colours(t).0))
}

/// A switch in the muted accent. A click publishes `message` with the new
/// state.
pub fn switch<'a, M: 'static>(on: bool, message: impl Fn(bool) -> M + 'a) -> Element<'a, M> {
    toggler(on).on_toggle(message).into()
}

/// A switch in the muted accent, disabled without a message.
pub fn switch_maybe<'a, M: 'static>(
    on: bool,
    message: Option<impl Fn(bool) -> M + 'a>,
) -> Element<'a, M> {
    toggler(on).on_toggle_maybe(message).into()
}

/// A settings row with a switch in the muted accent: libcosmic's
/// `settings::item::builder(title).toggler(on, message)` and
/// `.toggler_maybe(on, message)`, with the same row that a click anywhere
/// flips. Import this and write `.switch` and `.switch_maybe` instead.
pub trait SwitchItem<'a, M> {
    /// libcosmic's `.toggler`, in the muted accent
    fn switch(self, on: bool, message: impl Fn(bool) -> M + 'static) -> ListButton<'a, M>;

    /// libcosmic's `.toggler_maybe`, in the muted accent: disabled without a
    /// message
    fn switch_maybe(
        self,
        on: bool,
        message: Option<impl Fn(bool) -> M + 'static>,
    ) -> ListButton<'a, M>;
}

impl<'a, M: Clone + 'static> SwitchItem<'a, M> for Item<'a, M> {
    fn switch(self, on: bool, message: impl Fn(bool) -> M + 'static) -> ListButton<'a, M> {
        let press = message(!on);
        list::button(self.control(switch(on, message))).on_press(press)
    }

    fn switch_maybe(
        self,
        on: bool,
        message: Option<impl Fn(bool) -> M + 'static>,
    ) -> ListButton<'a, M> {
        let press = message.as_ref().map(|message| message(!on));
        list::button(self.control(switch_maybe(on, message))).on_press_maybe(press)
    }
}
