// SPDX-License-Identifier: GPL-3.0-only

//! The household apps' shared libcosmic widgets: "use shared code as shared
//! UI component code to prevent having to maintain multiple versions of the
//! same code and having inconsistent UI" (Juliet, 24 Oct).
//!
//! - [`switch()`], [`switch_maybe`] and [`SwitchItem`]: libcosmic's switch
//!   in the muted accent (household rule 6).
//! - [`muted`]: any libcosmic widget drawn with the muted accent in place
//!   of the accent, and [`muted_colours`], the two colours themselves.
//!
//! Plain colour maths stays in household-kit's `household-tones`; this
//! crate turns it into libcosmic's types.

mod muted;
mod switch;

pub use muted::{muted, muted_colours};
pub use switch::{SwitchItem, switch, switch_maybe};

#[cfg(test)]
mod switch_tests;
