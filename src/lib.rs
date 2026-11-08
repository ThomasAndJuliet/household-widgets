// SPDX-License-Identifier: GPL-3.0-only

//! The household apps' shared libcosmic widgets: "use shared code as shared
//! UI component code to prevent having to maintain multiple versions of the
//! same code and having inconsistent UI" (Juliet, 24 Oct).
//!
//! - [`switch()`], [`switch_maybe`], [`toggler()`] and [`SwitchItem`]:
//!   libcosmic's switch in the muted accent (household rule 6).
//! - [`muted_colours`]: the muted accent of a theme, and what goes on it.
//!
//! Plain colour maths stays in household-kit's `household-tones`; this
//! crate turns it into libcosmic's types.

#[macro_use]
mod forward;
pub mod layout;
mod level;
pub mod menu;
mod switch;
mod text_button;
pub mod text_menu;
mod tooltip;

pub use level::{LEVEL_CAP, Level};
pub use switch::{SwitchItem, muted_colours, switch, switch_maybe, toggler};
pub use text_button::{BUTTON_HEIGHT, Tertiary, TextButton, button_padding};
pub use text_menu::{TextMenu, TextMenuWords, text_menu};
pub use tooltip::{TIP_WIDTH, link_tooltip, tip_bubble, tip_padding, tooltip_unless, tooltip_with};

#[cfg(test)]
mod switch_tests;
