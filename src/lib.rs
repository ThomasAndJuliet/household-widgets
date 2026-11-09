// SPDX-License-Identifier: GPL-3.0-only

//! The household apps' shared libcosmic widgets: "use shared code as shared
//! UI component code to prevent having to maintain multiple versions of the
//! same code and having inconsistent UI" (Juliet, 24 Oct).
//!
//! - [`switch()`], [`switch_maybe`], [`toggler()`] and [`SwitchItem`]:
//!   libcosmic's switch in the muted accent (household rule 6).
//! - [`muted_colours`]: the muted accent of a theme, and what goes on it.
//! - [`text_box`]: a text box that does what a text box does: [`TextBox`],
//!   iced's text and caret with an undo history, its standard keys and
//!   Shift+click, and its selection drawn dimmed while unfocused.
//! - [`short_text_box`]: the one-line text box, [`ShortTextBox`] and its
//!   widget, sharing the text box's history, steps and keys.
//!
//! Plain colour maths stays in household-kit's `household-tones`; this
//! crate turns it into libcosmic's types.

#[macro_use]
mod forward;
#[doc(hidden)]
pub mod history;
mod kept;
pub mod layout;
mod level;
pub mod look;
pub mod menu;
mod scroll;
pub mod short_text_box;
mod switch;
pub mod tabs;

pub mod text_box;
mod text_button;
pub mod text_menu;
mod tooltip;

pub use history::{Turn, Undoable};
pub use level::{LEVEL_CAP, Level};
pub use look::Look;
pub use scroll::horizontal_scroll;
pub use short_text_box::{PasswordBox, ShortTextBox};
pub use switch::{SwitchItem, muted_colours, switch, switch_maybe, toggler};
pub use text_box::TextBox;
pub use text_button::{BUTTON_HEIGHT, Tertiary, TextButton, button_padding};
pub use text_menu::{TextMenu, TextMenuHistory, TextMenuWords, text_menu};
pub use tooltip::{TIP_WIDTH, link_tooltip, tip_bubble, tip_padding, tooltip_unless, tooltip_with};

#[cfg(test)]
mod short_text_box_tests;
#[cfg(test)]
mod switch_tests;
#[cfg(test)]
mod text_box_tests;
