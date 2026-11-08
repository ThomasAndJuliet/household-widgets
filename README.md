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
| `text_box::editor(&text_box, on_action)` | libcosmic's `text_editor` over the box: its actions, the standard keys, and the colours of a kept selection; set the rest (`.id`, `.placeholder`, `.padding`, an app's own `.key_binding`) on it |
| `text_box::wrap(content, &text_box, on_action)` | round the editor (and its text menu): Shift+click extends the selection, and the selection kept while the box is unfocused is drawn dimmed |
| `text_box::shift_click(editor, extend)` | Shift+click alone, for a box with an editor of its own |

A new draft is a new `TextBox` (the terminal's prompt, once sent). The history keeps 300 steps and at most 8 MB of text.

**A selection kept while unfocused is drawn dimmed**, as GTK draws a selection in a box that is not the focus: what the menu's Cut and Copy act on is always seen. Neither iced's editor nor libcosmic's input draws a selection without the focus, so `wrap` (and the short text box's widget) draws the box from a second widget tree, its state as when focused, while the box's colours give the kept look: the household's selection tint, the accent at 32 % over the box's background (rule 6, "What is highlighted, and how"), with the box's text colour moved only as far as the fill rule needs (`household_tones::on_tint`), in place of the solid accent of a box being typed into. `text_box::kept_selection_colours(theme, text, beneath)` gives the pair. Nothing of libcosmic changes.

### The short text box

"Yes we should have a "short text box" widget and much of the code can be shared between the text box and short text box but there are also obviously different" (Juliet, 8 Nov 2026). Module `short_text_box`: a one-line field over libcosmic's `text_input`, with the text box's history.

| Call | Is |
| --- | --- |
| `ShortTextBox::new()`, `ShortTextBox::starting_with(text)` | a field's value with its undo history, nothing to undo yet; it derefs to the `str`, and `.value()`. A value that is where the field starts (a page number shown as the pages move) is a new box |
| `ShortTextBox::password()` | a password's box: it keeps no history, as in GTK |
| `short.perform(action)` | does a `short_text_box::Action`: the field's `Edit`, `Undo`, `Redo`; a step with the app's part hands it back as `Turn` |
| `short.replace(text, caret)`, `short.replace_with(text, caret, part)` | the app's change of the value (clearing a search): one step Ctrl+Z takes back; the field puts its caret at `caret` |
| `short.undo_part()`, `short.redo_part()`, `short.forget_parts()`, `short.can_undo()`, `short.can_redo()` | as the text box's |
| `short_text_box::text(&short, placeholder, on_action)` | the field, as libcosmic's `text_input` |
| `short_text_box::search(&short, placeholder, on_action)` | as `search_input`; `.on_clear(message)` shows its clear button, and the app clears the box with `replace` |
| `short_text_box::secure(&short, placeholder, on_action, hidden, on_toggle)` | as `secure_input`, with its show-or-hide button when `on_toggle` is given |
| `.on_submit(message)`, `.id(..)`, `.width(..)`, `.padding(..)`, `.size(..)` | on the field: Enter publishes `message`; the rest as `text_input`'s |

The field is `text_menu(field, &words, style).history(TextMenuHistory::of(&short, on_action))` for its menu with Undo · Redo, as a text box's editor is; a password's has no history to offer.

**Shared with the text box, one copy each:** the history and its bounds, the rules that make its steps (a run of typing ended by moving the caret, by a space after a word, by changing between typing and deleting; a paste, a cut, a word deleted and the app's change a step each; typing over a selection one step with the typing after it), the app's part of a step (rule 19b), `Caret`, `standard_keys()` and its key matching, the text menu's Undo · Redo (`TextMenuHistory::of` takes either box), and the kept selection's look and drawing.

**Different, as one-line fields are:**

- The app keeps the value, as with `text_input`. The caret is the widget's, so each `Edit` the field reports says where the caret was and is, and the key that made it; an Undo puts back the caret and selection by asking the field, which does it when it is next built (the app returns no task).
- Enter submits; there are no new lines.
- Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y are taken only while the field is focused; otherwise the key goes on to the app (Marginalium's Ctrl+Z undoes its notes). A password field takes them and does nothing.
- Ctrl+Backspace and Ctrl+Delete are libcosmic's own, by words (to the start or end in a password), so they are not written again here; Shift+click, which `text_input` lacks, is added.
- libcosmic's input lets its selection go on Esc and Tab and keeps it when a press elsewhere takes the focus (a menu opened): that kept selection is the one drawn dimmed.

**The kinds** are libcosmic's own constructors, one function each, giving one widget, `Field`: the look of each stays libcosmic's, and everything else is the same code.

## Depending on it

```toml
household-widgets = { git = "ssh://git@github.com/ThomasAndJuliet/household-widgets.git" }
```

The crate depends on libcosmic as an app does, by upstream's URL with no rev:

```toml
libcosmic = { git = "https://github.com/pop-os/libcosmic.git", default-features = false }
```

Every household app has `[patch."https://github.com/pop-os/libcosmic.git"]` pointing at the household fork at the revision it pins. A patch replaces every dependency on that URL, so this crate is built with the app's own libcosmic: there is one libcosmic in the app's lock, selected by the app at a compatible fork revision. Naming the fork's URL here would give an app two libcosmics, and its `Element` would not be this crate's `Element`. The same goes for `household-tones`: this crate and the app must spell its URL the same way, as above, so that the app's lock has one.

This repository's own `Cargo.toml` has a `[patch]` for its tests, Clippy and CI. Apps using `TextMenu` must pin a compatible household libcosmic revision providing the native text target and session interfaces. This crate is tested against `cb889ef4bbfccd45739d5fa6f9140fd0e53df9fa`, which includes the native editing, popup and named-selection fixes; older fork revisions lack these interfaces or fixes. Cargo reads `[patch]` only from the workspace being built, so each app chooses its own compatible fork revision.

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
