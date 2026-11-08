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
so its palette and any faded-window state remain under its control. `Tertiary`
accepts `.leading_element(element)` for a custom mark (which takes precedence
over `.leading_icon(...)`), `.height(32.0)` for a 32 px target, and
`.icon_only(true)` for a square target showing only that leading mark. Its
original label remains the native accessible name when the `a11y` feature is
enabled. The default height and labelled layout remain unchanged. `Level<M>`
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
native command rows; `.actions(menu)` adds resolved application actions, and
`.history(TextMenuHistory::of(&text_box, on_action))` a text box's Undo · Redo
after Select all, publishing the box's own messages. With text selected the
menu starts with Copy (rule 28). `resolved_menu` gives the rows for a context,
for an app's tests. The
widget snapshots those messages when opened, preserves native editing and
selection, and presents disabled explanations from native capabilities. Put
caret or tooltip wrappers outside the resulting element. Enable this crate's
`a11y` feature whenever the app enables libcosmic accessibility, so the
wrapped target contributes its accessibility nodes. The app's libcosmic
patch must also resolve `iced_accessibility` to the same fork revision.

`layout::responsive(max_level, view)` measures native representations with the
actual renderer and chooses the first permitted level that fits. The factory
receives `layout::Level::{Preferred, Mild, Strong, AllPermitted}`. A parent can
raise the permitted level through its own simple layout stages. The first native
adapter handles one fixed arrangement with increasing levels; it does not try
different arrangements or schedule individual children. `.width(..)` and
`.height(..)` set its allocation; the defaults are fill width and shrink height.
Factories must have no application side effects: measurements create and discard
fresh elements and state trees, with the runtime's named states isolated.

The pure `layout::first_fitting(max_level, available_size, measure)` query returns
`None` when no representation fits. The native adapter then gives the last
permitted representation the available constraints; this fallback does not claim
it fits. `layout::allocate_width(available, capacities)` shares a shortage at each
child's original `weight * (preferred - minimum)` rate, clamps at minima, and
redistributes using the remaining original rates. Higher weights shrink faster;
zero protects preferred width. It leaves surplus unallocated and returns an
explicit error if the lower bounds cannot fit.

Selected controls use native tree reconciliation, retaining compatible focus
and open inline dropdowns on resize. Replacing a row with a dropdown discards
incompatible native state and its inline overlay; focus is not transferred
between control types. External surface popups require their owner's lifecycle
handling. Native dropdown accessibility and operation support remain limited by
libcosmic; the adapter forwards the selected control's existing capabilities.
Enable the shared `a11y` feature when the app enables libcosmic accessibility.

For nested layouts, use `layout::child(view)` for native leaves and
`layout::row_child(children, spacing)` for composites, then put one
`layout::responsive_child(max_level, root)` at the root. A `Child` exposes
`query(level, renderer)` returning preferred and minimum sizes, and `view(level)`
for that exact level. Every nested row passes the parent's candidate level to
its descendants; only the root chooses a level. Rows allocate their measured
child boxes with the shared capacity allocator. `.weight(0.0)` protects a fixed
control's preferred width. `.minimum_width(..)` is for views such as scrolling
viewports that remain usable at their queried height when width shrinks.

`tabs::Model::new(items, selected)` owns stable native tab entities and an ID.
Each `tabs::Tab` supplies its label and icon. Keep the model's selection derived
from the app's canonical section by calling `select(index)` whenever it changes.
`tabs::strip(&model, policy, on_select_index)` returns one queryable child; its
parent knows no tab labels or icons. The default height is 44; use
`.button_height(32)` for a native header slot, or `.font_size(..)` for other live
typography. Measurements, rendering, hover names and selection reveal use the
same native tab builder and actual renderer.

`tabs::Overflow::Dropdown` uses labels at levels 0/1, the active label and icons
at 2, and a named dropdown at 3. Inactive compact tabs keep native hover tooltips;
the shared presentation wrapper delegates tab events and keyboard operations.
`Overflow::Scroll { minimum_viewport }` keeps all labels and permits a scrolling
viewport from level 1. Its minimum viewport is distinct from its full content
width. An embedded scrollbar has its own 8px lane plus a 2px gap below the tabs.
The query therefore reports button height plus 10px, including when wide, so
scrolling does not change the row height. A 32px header cannot fit this form
without allocating more height or choosing a different overflow policy. Selection
changes and viewport resizing reveal the selected tab using measured native tab
bounds, while ordinary manual scrolling is retained. The native tab style is
the terminal's TabBar style with plain inactive title colours on hover and press.
Native segmented tabs currently expose no accessibility nodes or per-item name
API; that upstream limitation also applies through this component.
Native keyboard navigation remains delegated; revealing an offscreen focused
tab before activation needs a native focus-bounds hook that libcosmic does not
currently expose. Selecting it reveals it through the normal selection path.

The text box: "You were asked to implement a input text box. This obviously includes the usual functionality of such a box which includes ctrl + z" (Juliet, 6 Nov 2026). iced's `text_editor` with what it lacks: an undo history, Ctrl+Backspace and Ctrl+Delete, and Shift+click; iced already has the rest of the standard keys. Module `text_box`; its documentation has the steps Ctrl+Z goes back by.

| Call | Is |
| --- | --- |
| `TextBox::new()`, `TextBox::starting_with(text, caret)` | a box's text and caret with its undo history, nothing to undo yet; it derefs to iced's `Content`, so `text_editor(&text_box)`, `.text()`, `.selection()`. `caret` is a `text_box::Caret`: `Start`, `End`, or `At(cursor)` |
| `text_box.perform(action)` | does a `text_box::Action`: iced's own (`text_editor::Action` converts into it), `Undo`, `Redo`, `DeleteWordBefore`, `DeleteWordAfter`, `Extend` |
| `text_box.replace(text, caret)` | the app's change of the whole text: one step Ctrl+Z takes back |
| `text_box.amend(text, caret)` | the same change again, part of the last step (the saves of a text open in another editor) |
| `text_box.replace_with(text, caret, part)` | one step that is more than the text: `part`, the app's own side of it (a card stashed), comes back from `perform` as `Turn::Undone(part)` or `Turn::Redone(part)` for the app to undo or redo; `TextBox<P>` holds parts of type `P` |
| `text_box.undo_part()`, `text_box.redo_part()`, `text_box.forget_parts()` | before Undo or Redo, the part it would hand back; if the app can no longer undo or redo such parts, the history ends at the nearest of them, the typing between kept |
| `text_box.can_undo()`, `text_box.can_redo()` | for a menu's Undo and Redo |
| `text_box::key_binding(press, on_action)` | the standard keys, for `.key_binding(..)`; an app with keys of its own handles them first and ends with this. Nothing while the box is unfocused, so the app keeps its own Ctrl+Z |
| `text_box::standard_keys()` | the keys a text box adds to iced's and what each does, the one list: the key binding goes by it, and an app shows it in its shortcuts and menus |
| `text_box::history_key(key, physical, modifiers)` | Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y alone, as an `Action`, for keys that reach a box other than as a focused widget |
| `text_box::shift_click(editor, extend)` | round the editor: Shift+click extends the selection |

A new draft is a new `TextBox` (the terminal's prompt, once sent). The history keeps 300 steps and at most 8 MB of text. One-line boxes (`text_input`) will get the same history over the value the app keeps, with a wrapper that takes the undo keys while the input is focused; a password box keeps none, as in GTK.

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

`layout::spacer(minimum)` reports a fixed minimum gap during queries and grows
into remaining row space. `Child::grow(weight)` shares surplus after preferred
widths fit; it is independent of the shrink weight. Nested components remain
explicit: give a composite a grow weight when its parent should offer it extra
space. An expanding gap therefore needs no application-specific width estimate.

`Child::round_width_up()` rounds only that component's measured width offers
up to whole logical pixels, for aligned control origins. It leaves the native
measurement function and the continuous allocation algorithm unchanged.
