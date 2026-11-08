// SPDX-License-Identifier: GPL-3.0-only

//! How a text box looks: household rule 25a, "Text boxes come in two
//! kinds" (Juliet, 8 Nov 2026, from `sheets/text-box-look-sheet-*.png`:
//! "the look should move into the shared widget but we have two cases a
//! prominant text box (like the composer) which uses a thicker border width
//! and the accent colour and normal text boxes which should be more
//! subtle"). One place for both boxes: [`crate::text_box::editor`] and
//! [`crate::short_text_box`]'s fields draw by it; apps do not style text
//! boxes themselves.
//!
//! | | at rest | hovered | focused | focused, window behind others |
//! |---|---|---|---|---|
//! | **Prominent** (a window's main input) | 4 px rest colour | as at rest | 4 px accent | 4 px accent faded halfway |
//! | **Normal** (every other box) | 1 px rest colour | 1 px rest colour a step towards the text | 2 px muted accent | 2 px rest colour |
//!
//! Both are rounded at libcosmic's small radius, on the page's colour. A
//! selection kept while the box is unfocused is drawn with the unfocused
//! border, and the tint (rule 6). The colour maths is household-tones'
//! (`rest_border`, `hovered_border`, `faded_accent`, `muted_accent`).

use cosmic::Theme;
use cosmic::iced::{Background, Border, Color};
use household_tones::{Mode, Role, Srgb, faded_accent, hovered_border, rest_border};

/// Which of the two kinds of text box, and whether its window is behind
/// others. A window knows the second (it is drawn with its own theme while
/// another has the keys): the app says it per box, as each window's view is
/// built, with [`Look::behind`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    prominent: bool,
    behind: bool,
}

/// A box's state, as its border shows it
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Rest,
    Hovered,
    Focused,
}

impl Look {
    /// A normal text box: every box but a window's main input
    pub fn normal() -> Self {
        Self::default()
    }

    /// The prominent text box: a window's main input (the terminal's
    /// prompt, a chat's message box)
    pub fn prominent() -> Self {
        Self {
            prominent: true,
            behind: false,
        }
    }

    /// Whether the box's window is behind others
    pub fn behind(self, behind: bool) -> Self {
        Self { behind, ..self }
    }

    /// The box's border in `state`
    pub(crate) fn border(self, theme: &Theme, state: State) -> Border {
        let cosmic = theme.cosmic();
        let page = page(theme);
        let rest = colour(rest_border(
            rgba(cosmic.bg_divider().into()),
            cosmic.is_dark,
        ));
        let (width, color) = match (self.prominent, state) {
            (true, State::Focused) => {
                let accent: Color = cosmic.accent_color().into();
                let colour = match self.behind {
                    true => opaque(faded_accent(rgba(accent), rgba(rest), srgb(page))),
                    false => accent,
                };
                (prominent_width(theme), colour)
            }
            (true, _) => (prominent_width(theme), rest),
            (false, State::Rest) => (1.0, rest),
            (false, State::Hovered) => {
                let text: Color = cosmic.on_bg_color().into();
                let on_page = flat(rest, page);
                (1.0, opaque(hovered_border(srgb(on_page), srgb(text))))
            }
            (false, State::Focused) => match self.behind {
                true => (2.0, rest),
                false => (2.0, crate::muted_colours(theme).0),
            },
        };
        Border {
            width,
            color,
            radius: cosmic.corner_radii.radius_s.into(),
        }
    }
}

/// The prominent box's border width: libcosmic's smallest space, 4 px by
/// default
fn prominent_width(theme: &Theme) -> f32 {
    f32::from(theme.cosmic().space_xxxs())
}

/// What a text box's field is filled with: the page's colour (rule 25d's
/// light tone), or a translucent theme's own background
pub(crate) fn page(theme: &Theme) -> Color {
    let cosmic = theme.cosmic();
    if theme.transparent {
        return cosmic.background(true).base.into();
    }
    let mode = match cosmic.is_dark {
        true => Mode::Dark,
        false => Mode::Light,
    };
    opaque(Role::Light.tone(mode).expect("the light tone"))
}

/// The page's colour as a box's background
pub(crate) fn background(theme: &Theme) -> Background {
    Background::Color(page(theme))
}

fn rgba(c: Color) -> household_tones::Srgba {
    household_tones::Srgba::new(c.r, c.g, c.b, c.a)
}

fn srgb(c: Color) -> Srgb {
    Srgb::new(c.r, c.g, c.b)
}

fn colour(c: household_tones::Srgba) -> Color {
    Color::from_rgba(c.r, c.g, c.b, c.a)
}

fn opaque(c: Srgb) -> Color {
    Color::from_rgb(c.r, c.g, c.b)
}

/// `top` as it shows over `under`
fn flat(top: Color, under: Color) -> Color {
    crate::kept::flat(Background::Color(top), under)
}
