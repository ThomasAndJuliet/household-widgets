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

The muted switch, rule 6: a switch that is on uses the muted accent ("yes use the muted accent for them all", Juliet, 30 Oct). Everything but the "on" fill is libcosmic's.

| Call | Is |
| --- | --- |
| `switch(on, message)` | a switch; `message` gets the new state |
| `switch_maybe(on, Some(message))` | the same, disabled with `None` |
| `muted(content)` | any widget drawn with the muted accent in place of the accent, e.g. `muted(widget::toggler(on).label(label).spacing(8).on_toggle(message))` |
| `item.switch(on, message)`, `item.switch_maybe(on, message)` | libcosmic's `settings::item::builder(title).toggler(..)` and `.toggler_maybe(..)`, the whole row clickable; import `household_widgets::SwitchItem` |
| `muted_colours(&theme)` | the muted accent and the colour of text and icons on it |

## Depending on it

```toml
household-widgets = { git = "ssh://git@github.com/ThomasAndJuliet/household-widgets.git" }
```

The crate depends on libcosmic as an app does, by upstream's URL with no rev:

```toml
libcosmic = { git = "https://github.com/pop-os/libcosmic.git", default-features = false }
```

Every household app has `[patch."https://github.com/pop-os/libcosmic.git"]` pointing at the household fork at the revision it pins. A patch replaces every dependency on that URL, so this crate is built with the app's own libcosmic: there is one libcosmic in the app's lock, and there is no version tie between this crate and any app. Naming the fork's URL here would give an app two libcosmics, and its `Element` would not be this crate's `Element`. The same goes for `household-tones`: this crate and the app must spell its URL the same way, as above, so that the app's lock has one.

This repository's own `Cargo.toml` also has a `[patch]`, pinned to the fork at `183fe5a8`, the revision more apps pin than any other. Cargo reads `[patch]` only from the workspace being built, so it applies only to this repository's tests, Clippy and CI. Move it when the apps move.

An app's CI needs its own read key for this repository, `HOUSEHOLD_WIDGETS_DEPLOY_KEY`, with a line for it in that app's `scripts/ci-private-crates.sh`, and a Flatpak app needs the crate in its `cargo-sources.json`.

## Adding a widget

1. Move the best of the apps' copies here, one module per widget, and keep its behaviour unless a rule says otherwise. Work out what every app's call sites use first, so that each changes by a line.
2. Take colours from `household-tones`. If the maths is missing, add it there.
3. Add tests that draw the real widget with iced's headless tiny-skia renderer and read the pixels, as `src/switch_tests.rs` does: the expected look comes from the rule's own values, or from libcosmic's widget drawn beside it.
4. Name the rule and Juliet's words in the module's documentation.
5. Build against the oldest and newest libcosmic the apps pin. `Widget` and the theme can change between them.

## Checks and CI

```sh
~/.local/bin/neo-cargo ~/Code/household-widgets household-widgets -- fmt --check
~/.local/bin/neo-cargo ~/Code/household-widgets household-widgets -- clippy --all-targets --locked -- -D warnings
~/.local/bin/neo-cargo ~/Code/household-widgets household-widgets -- test --locked
```

Neo cannot fetch the private repositories over ssh, so the revisions in `Cargo.lock` must already be in neo's cargo cache. To add one, push it there from a clone, e.g. `git push jcooke@neo:.cargo/git/db/household-kit-342368da86f19946 REV:refs/heads/locked-REV`; the fork's is `libcosmic-bd0cc8a7924f73dc`.

`.github/workflows/ci.yml` runs the same three checks on neo's runner (`neo-ubuntu`, or GitHub-hosted when the `CI_RUNNER` variable is `hosted`). It fetches the private crates with `scripts/ci-private-crates.sh` (COSMIC Terminal's, copied), so the repository needs two secrets, the private halves of read-only deploy keys: `LIBCOSMIC_DEPLOY_KEY` for the fork and `HOUSEHOLD_KIT_DEPLOY_KEY` for household-kit. Without them, the job stops at that step with a message naming the secret.

## Licence

GPL-3.0-only, because the code comes from Cerea, which is under that licence.
