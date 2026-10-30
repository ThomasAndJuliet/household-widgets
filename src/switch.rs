// SPDX-License-Identifier: GPL-3.0-only

//! libcosmic's switch in the muted accent: "yes use the muted accent for
//! them all" (Juliet, 30 Oct; household rule 6). Everything but the "on"
//! fill is libcosmic's: size, handle, animation, hover, disabled.
//!
//! For a switch with a label or other options, wrap libcosmic's own in
//! [`muted`], as [`switch()`] does.

use cosmic::widget::list::{self, ListButton};
use cosmic::widget::settings::item::Item;
use cosmic::{Element, widget};

use crate::muted;

/// A switch in the muted accent. A click publishes `message` with the new
/// state.
pub fn switch<'a, M: 'static>(on: bool, message: impl Fn(bool) -> M + 'a) -> Element<'a, M> {
    muted(widget::toggler(on).on_toggle(message))
}

/// A switch in the muted accent, disabled without a message.
pub fn switch_maybe<'a, M: 'static>(
    on: bool,
    message: Option<impl Fn(bool) -> M + 'a>,
) -> Element<'a, M> {
    muted(widget::toggler(on).on_toggle_maybe(message))
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
