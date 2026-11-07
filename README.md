# household-widgets

The household apps' shared libcosmic widgets. Juliet, 24 Oct 2026: "use shared code as shared UI component code to prevent having to maintain multiple versions of the same code and having inconsistent UI".

**The rule:** a widget used by more than one app lives here, and those apps call it. A widget used by one app stays in that app until a second app needs it.

What goes where (household-ui `RULES.md`, memory `shared-ui-code`):

| Kind of code | Where |
| --- | --- |
| A widget, or anything with libcosmic types | here |
| Plain colour maths, no libcosmic types | household-kit's `household-tones` |
| A small bug fix in libcosmic | the household fork, ThomasAndJuliet/libcosmic |
| Drawing only one app does (the terminal grid, an editor's syntax) | that app |

## What is here

The muted switch, rule 6: a switch that is on uses the muted accent ("yes use the muted accent for them all", Juliet, 30 Oct). It is libcosmic's toggler with the toggler class `theme::Toggler::accent`, so only the "on" fill is ours; hover and disabled follow from it as they do from the accent.

| Call | Is |
| --- | --- |
| `switch(on, message)` | a switch; `message` gets the new state |
| `switch_maybe(on, Some(message))` | the same, disabled with `None` |
| `toggler(on)` | libcosmic's `widget::toggler` in the muted accent, for a switch with options, e.g. `toggler(on).label(label).spacing(8).on_toggle(message)` |
| `item.switch(on, message)`, `item.switch_maybe(on, message)` | libcosmic's `settings::item::builder(title).toggler(..)` and `.toggler_maybe(..)`, the whole row clickable; import `household_widgets::SwitchItem` |
| `muted_colours(&theme)` | the muted accent and the colour of text and icons on it |

The shared tooltip functions own the 320 px wrap limit, theme spacing, bounded
bubble, cursor-following link bubble, and stable widget shape when a bubble is
hidden. An app supplies its body-text element to `tooltip_with`, and can use
`tooltip_unless` when a menu or list covers the bubble.

`TextButton<M>` and `Tertiary<M>` own the text, icon, row and button layout for
the household's three button levels. An app passes its `theme::Button` class,
so its palette and any faded-window state remain under its control. `Level<M>`
aligns a settings-row control with the first two text lines; its body-line
callback is read during layout, so a changed interface size takes effect.

`menu::Menu<M>` holds resolved menu rows, with app messages, icon handles,
shortcuts, marks and disabled reasons. The app supplies bundled mark icons and
its body-text constructor in `MenuStyle` when converting the menu into native
trees. `width()` includes submenus; `map()` adapts messages without rebuilding
menu metadata. `MenuRow` also supplies the row contents and measured width
for native menu-bar adapters; `measured_width` shares its bounded shaping cache
with other interface text at any size and font.

`text_menu(target, &words, style)` wraps a native text input, editor or
selectable text. Supply localized `TextMenuWords` and a `MenuStyle` for the
native command rows; `.actions(menu)` adds resolved application actions. The
widget snapshots those messages when opened, preserves native editing and
selection, and presents disabled explanations from native capabilities. Put
caret or tooltip wrappers outside the resulting element. Enable this crate's
`a11y` feature whenever the app enables libcosmic accessibility, so the
wrapped target contributes its accessibility nodes. The app's libcosmic
patch must also resolve `iced_accessibility` to the same fork revision.

## Depending on it

```toml
household-widgets = { git = "ssh://git@github.com/ThomasAndJuliet/household-widgets.git" }
```

The crate depends on libcosmic as an app does, by upstream's URL with no rev:

```toml
libcosmic = { git = "https://github.com/pop-os/libcosmic.git", default-features = false }
```

Every household app has `[patch."https://github.com/pop-os/libcosmic.git"]` pointing at the household fork at the revision it pins. A patch replaces every dependency on that URL, so this crate is built with the app's own libcosmic: there is one libcosmic in the app's lock, selected by the app at a compatible fork revision. Naming the fork's URL here would give an app two libcosmics, and its `Element` would not be this crate's `Element`. The same goes for `household-tones`: this crate and the app must spell its URL the same way, as above, so that the app's lock has one.

This repository's own `Cargo.toml` has a `[patch]` for its tests, Clippy and CI. Apps using `TextMenu` must pin a compatible household libcosmic revision providing the native text target and session interfaces. This crate is tested against `dbbbcfa31dd80f3e4b23f2631ce1c9444bc3adbc`, which includes the native editing, popup and named-selection fixes; older fork revisions lack these interfaces or fixes. Cargo reads `[patch]` only from the workspace being built, so each app chooses its own compatible fork revision.

An app's CI needs its own read key for this repository, `HOUSEHOLD_WIDGETS_DEPLOY_KEY`, with a line for it in that app's `scripts/ci-private-crates.sh`, and a Flatpak app needs the crate in its `cargo-sources.json`.

## Adding a widget

1. Move the best of the apps' copies here, one module per widget, and keep its behaviour unless a rule says otherwise. Work out what every app's call sites use first, so that each changes by a line.
2. Take colours from `household-tones`. If the maths is missing, add it there.
3. Add tests that draw the real widget with iced's headless tiny-skia renderer and read the pixels, as `src/switch_tests.rs` does: the expected look comes from the rule's own values, or from libcosmic's widget drawn beside it.
4. Name the rule and Juliet's words in the module's documentation.
5. Build against the supported libcosmic revisions. `TextMenu` requires the native text target and session interfaces; older revisions cannot build it. `Widget` and the theme can change between revisions.

## Checks and CI

```sh
~/.local/bin/neo-cargo ~/Code/household-widgets household-widgets -- fmt --check
~/.local/bin/neo-cargo ~/Code/household-widgets household-widgets -- clippy --all-targets --all-features --locked -- -D warnings
~/.local/bin/neo-cargo ~/Code/household-widgets household-widgets -- test --locked
~/.local/bin/neo-cargo ~/Code/household-widgets household-widgets -- test --all-features --locked
```

Neo cannot fetch the private repositories over ssh, so the revisions in `Cargo.lock` must already be in neo's cargo cache. To add one, push it there from a clone, e.g. `git push jcooke@neo:.cargo/git/db/household-kit-342368da86f19946 REV:refs/heads/locked-REV`; the fork's is `libcosmic-bd0cc8a7924f73dc`.

`.github/workflows/ci.yml` runs `cargo fmt --all --check`,
`cargo clippy --all-targets --all-features --locked -- -D warnings`,
`cargo test --locked` and `cargo test --all-features --locked` on neo's runner (`neo-ubuntu`, or GitHub-hosted when the `CI_RUNNER` variable is `hosted`). It fetches the private crates with `scripts/ci-private-crates.sh` (COSMIC Terminal's, copied), so the repository needs two secrets, the private halves of read-only deploy keys: `LIBCOSMIC_DEPLOY_KEY` for the fork and `HOUSEHOLD_KIT_DEPLOY_KEY` for household-kit. Without them, the job stops at that step with a message naming the secret.

## Licence

GPL-3.0-only, because the code comes from Cerea, which is under that licence.
