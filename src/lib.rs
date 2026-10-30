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

mod switch;

pub use switch::{SwitchItem, muted_colours, switch, switch_maybe, toggler};

#[cfg(test)]
mod switch_tests;
