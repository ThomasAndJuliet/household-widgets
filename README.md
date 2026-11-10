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

The shared layout engine separates component forms from the parent's stage
order. `layout::Stage::new(level, content)` requests one exact level and one
arrangement; `layout::stages(vec![...])` tries the stages in the supplied order.
For example, a policy can try a preferred row, a mildly compact row, then a
mildly compact column. Another can try the column before compaction. There is
no individual descendant scheduler or enumeration of child combinations.
Levels are `Preferred`, `Mild`, `Strong` and `AllPermitted`; the last respects
component restrictions and does not grant permission to hide.

Build native leaves with `layout::child(view)`, and composites with
`row_child(children, spacing)`, `column_child(children, spacing)` or
`wrap_child(children, spacing)`. Wrapping keeps immediate children atomic.
Every composite forwards the candidate level unchanged. `Child::view(level)`
also renders at that exact level; only the responsible root chooses a stage.
A component family can hide its implementation behind this same contract.
`forms(vec![(Level::Preferred, expanded_child), (Level::Strong, menu_child)])`
selects the family's own structured form: Mild inherits the expanded form,
but its descendants still receive Mild. This preserves composite continuous
ranges without requiring the parent to enumerate descendant alternatives.

`Child::query(level, renderer)` returns intrinsic preferred/minimum summaries.
`query_at(level, renderer, available_size)` returns complete `GeometryPlan`
geometry or an allocation error. Native content is remeasured at its allocated
width with unbounded height, so wrapped text supplies its actual height and
cannot certify a fit by being vertically clipped. `.minimum_width(...)`
explicitly declares a useful width range; a floor above an intrinsic Fill
measurement raises preferred width too, rather than being silently lost.
For a native leaf that reports infinite intrinsic width, a finite useful floor
also supplies the initial probe width. This is a preferred offer, not a maximum:
allocated Fill content can still grow. NaN and other invalid measurements remain
errors. Allocated Fill content consumes its actual width, while intrinsic queries
remain compressed. The view must actually support its declared range. A column
measures children at their allocated widths before allocating vertical space;
wrapping includes all row heights and spacing. Both dimensions must fit.

`layout::choose_stage(&stages, &renderer, available_size)` returns a
`StagePlan` with stage index, level and recursive geometry, or `NoFit`.
Geometry includes immediate-child indices, positions and allocated boxes;
hidden children are omitted. Each stage starts from its own preferred sizes,
so width released by menu compaction can restore a sibling title. The chosen
plan is used to render, rather than recomputing row membership during commit.
Native leaf dimensions are checked again during commit. If live widget state
causes disagreement with the isolated query, the root drops that rejected
event/overlay/accessibility tree and tries the next declared stage. It reports
no fit only after exhausting the stages.

`ResponsiveLevels::selection()` exposes the last selection result. With no
fit, the default native content is empty; `.fallback(|| element)` provides an
explicit alternative while the result remains `Err`. This intentionally
replaces the old implicit last-level/clipped fallback. The standalone
`first_fitting(...)` query remains available. The compatibility conveniences
`responsive(max_level, view)`, `responsive_child(max_level, root)` and
`row(max_level, children, spacing)` all use the same staged engine with a
successive-level policy. `.width(...)` and `.height(...)` set the root's
allocation; defaults are fill width and shrink height.

`allocate_width(available, capacities)` shares a shortage at each child's
original `weight * (preferred - minimum)` rate, clamps at minima and
redistributes using remaining original rates. Higher weights shrink faster;
zero protects preferred size. It leaves surplus unallocated and returns an
explicit error when lower bounds do not fit. Rows and columns reuse that
allocator on their respective allocation axes. Internal child weights and the
outward composite weight remain separate. `.grow(...)` shares width surplus;
`.grow_height(...)` shares height surplus in a column. Growth is separate from
shrink. `spacer(minimum)` supplies an elastic horizontal gap, and optional
`.round_width_up()` rounds a child's width offers without changing native
measurement or the continuous allocator.

`Child::map_view(|element| ...)` adds geometry-preserving styling or alignment
at a component boundary, retaining structured queries and useful ranges.
Decorators must not add padding, change dimensions or overflow their box;
native commit checks the decorated component as well as its leaves. Queries
do not invoke decorators, and undecorated controls keep their original native
state shape. The shared `Level` alignment finds label/caption lines through
vertically transparent allocation wrappers and forwards accessibility.

`horizontal_scroll(content)` gives an unavoidably wide control an explicit
native horizontal viewport. Its embedded scrollbar has a separate shared lane,
reserved with or without overflow, so text and controls cannot be covered by
the bar. Tabs use the same scrollbar metrics.

`.hidden_when(|level| ...)` is explicit component permission for a hidden
form. It contributes no slot, spacing, event target, overlay or accessibility
nodes. A composite whose descendants are all explicitly hidden also leaves
no phantom outer gap. This is omission from the presentation, not a guarantee
of retained widget-local focus/history; keep canonical state outside the form.
`scroll_child(view, minimum_outer_size)` declares a native scrolling viewport.
Its view implements scrolling and includes its controls/scrollbar lane; its
minimum describes the whole useful viewport. The engine can allocate viewport
width/height without fitting the full content extent. If the native scrollable
reports an infinite intrinsic dimension, the declared minimum viewport supplies
that offer after a finite native probe verifies it. Scrolling is never an
invented global fallback or an excuse to clip a non-scrollable child.

Factories must have no application side effects: queries build disposable
native elements and state trees with the runtime's named states isolated.
Selected controls use ordinary tree reconciliation, retaining compatible focus
and open inline dropdowns on resize. Replacing a control type discards
incompatible native state; focus is not automatically transferred. External
surface popups require their owner's lifecycle handling. Native accessibility
capabilities are forwarded; enable this crate's `a11y` feature whenever the app
enables libcosmic accessibility.

Queries currently use exact native measurements, not persistent geometry
caches or estimates. Cache invalidation, stale-plan protocols and automatic
window-minimum updates are separate work. The engine does not solve arbitrary
constraints or choose product policy: a declared stage deterministically
allocates its ranges, measures the result and checks it.

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
| `text_box.replace(text, caret)` | the app's change of the whole text: one step Ctrl+Z takes back; the same text again is no step |
| `text_box.amend(text, caret)` | the same change again, part of the last step (the saves of a text open in another editor) |
| `text_box.replace_with(text, caret, part)` | one step that is more than the text: `part`, the app's own side of it (a card stashed), comes back from `perform` as `Turn::Undone(part)` or `Turn::Redone(part)` for the app to undo or redo; `TextBox<P>` holds parts of type `P` |
| `text_box.undo_part()`, `text_box.redo_part()`, `text_box.forget_parts()` | before Undo or Redo, the part it would hand back; if the app can no longer undo or redo such parts, the history ends at the nearest of them, the typing between kept |
| `text_box.can_undo()`, `text_box.can_redo()` | for a menu's Undo and Redo |

`replace`, `replace_with`, the parts and `can_undo`/`can_redo` are the trait `household_widgets::Undoable`, written once for both boxes over one history (module `history`): import it to call them.
| `text_box::key_binding(press, on_action)` | the standard keys, for `.key_binding(..)`; an app with keys of its own handles them first and ends with this. Nothing while the box is unfocused, so the app keeps its own Ctrl+Z |
| `text_box::standard_keys()` | the keys a text box adds to iced's and what each does, the one list: the key binding goes by it, and an app shows it in its shortcuts and menus |
| `text_box::history_key(key, physical, modifiers)` | Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y alone, as an `Action`, for keys that reach a box other than as a focused widget |
| `text_box::editor(&text_box, look, on_action)` | libcosmic's `text_editor` over the box in the household look (`Look::prominent()` or `Look::normal()`, `.behind(window_behind)`): its actions, the standard keys, and the colours of a kept selection; set the rest (`.id`, `.placeholder`, `.padding`, an app's own `.key_binding`) on it, but no style |
| `text_box::styled(editor, look)` | an editor over text that is not a `TextBox`'s (a read-only source shown) in the same look |
| `look::State`, `Look::border(&theme, state)` | the border a kind draws in a state, for an app's tests to say what they expect |
| `text_box::wrap(editor, &text_box, on_action)` | round the editor, or the editor in its text menu, and nothing else (the type allows only those): Shift+click extends the selection, and the selection kept while the box is unfocused is drawn dimmed |
| `text_box::shift_click(editor, extend)` | Shift+click alone, for a box with an editor of its own |

A new draft is a new `TextBox` (the terminal's prompt, once sent). The history keeps 300 steps and at most 8 MB of text.

**A selection kept while unfocused is drawn dimmed**, as GTK draws a selection in a box that is not the focus: what the menu's Cut and Copy act on is always seen. Neither iced's editor nor libcosmic's input draws a selection without the focus, so `wrap` (and the short text box's widget) draws the box from a second widget tree, its state as when focused, while the box's colours give the kept look: the household's selection tint, the accent at 32 % over the box's background (rule 6, "What is highlighted, and how"), with the box's text colour moved only as far as the fill rule needs (`household_tones::on_tint`), in place of the solid accent of a box being typed into. `text_box::kept_selection_colours(theme, text, beneath)` gives the pair. Nothing of libcosmic changes.

### The look: two kinds of text box

Household rule 25a (Juliet, 8 Nov 2026, from the sheet in household-ui `sheets/text-box-look-sheet-*.png`: "the look should move into the shared widget but we have two cases a prominant text box (like the composer) which uses a thicker border width and the accent colour and normal text boxes which should be more subtle"). Module `look`, one place for both boxes; apps do not style text boxes.

| | at rest | hovered | focused | focused, window behind others |
|---|---|---|---|---|
| **Prominent** (a window's main input; the text box only) | 4 px rest colour | as at rest | 4 px accent | 4 px accent faded halfway |
| **Normal** (every other box and field) | 1 px rest colour | 1 px, the rest colour a 0.35 step towards the text | 2 px muted accent | 2 px rest colour |

Selected text on the solid accent is the text's own colour moved as far as reading needs, a grey going to the accent's on colour (rule 6, "What is drawn on a fill"). Both are rounded at libcosmic's `radius_s` and filled with the page's colour (rule 25d's light tone). A kept selection keeps the unfocused border. The rest colour is #5c5c5c on a dark theme and libcosmic's divider on a light one. The colour maths (`rest_border`, `hovered_border`, `faded_accent`, `muted_accent`) is household-tones'. An editable label (`short_text_box::editable`) keeps libcosmic's label look until it is edited.

Whether a window is behind others is the app's to say, per box, as it builds that window's view: `Look::behind(bool)` for an editor, `Field::behind(bool)` for a field. A theme-level value would not do: libcosmic draws every window with the theme of the window in front, which is also why an app whose windows have accents of their own (the terminal's tab colours) gives a prominent box its window's own with `Look::accent(colour)`.

### The short text box

"Yes we should have a "short text box" widget and much of the code can be shared between the text box and short text box but there are also obviously different" (Juliet, 8 Nov 2026). Module `short_text_box`: a one-line field over libcosmic's `text_input`, with the text box's history.

| Call | Is |
| --- | --- |
| `ShortTextBox::new()`, `ShortTextBox::starting_with(text)` | a field's value with its undo history, nothing to undo yet; it derefs to the `str`, and `.value()`. A value that is where the field starts (a page number shown as the pages move) is a new box |
| `ShortTextBox::sensitive()` | a box for values that are not secrets but must not linger (a card's name and expiry): Undo and Redo as ever, but the value it replaces, every step its history lets go of (past its bounds, Redo's when a new step is taken, parts forgotten) and all it holds when dropped are zeroized, and neither `Debug` nor its field's edits write its value out; `.is_sensitive()` |
| `PasswordBox::new()` | a password's value, with no history, as in GTK: `.perform(action)` (Undo and Redo do nothing), `.set(text, caret)`, `.take()` (the value handed over, the box empty again); it derefs to the `str` and writes nothing of it in `Debug`. Nothing of a password stays in memory it lets go of: each change zeroizes the value it replaces, in its own buffer, and dropping it zeroizes its value |
| `short.perform(action)` | does a `short_text_box::Action`: the field's `Edit`, `Undo`, `Redo`; a step with the app's part hands it back as `Turn` |
| `short.replace(text, caret)`, `short.replace_with(text, caret, part)` | the app's change of the value (clearing a search): one step Ctrl+Z takes back; the field puts its caret at `caret`, a `short_text_box::Caret` (`Start`, `End`, `At { anchor, caret }` in graphemes). The same value again by `replace` is nothing: no step, and the caret left where it is |
| `short.undo_part()`, `short.redo_part()`, `short.forget_parts()`, `short.can_undo()`, `short.can_redo()` | as the text box's (`Undoable`) |
| `short.perform_with(action, part)` | `perform`, a step the field's edit starts carrying the app's `part` (a search field's match, which its Undo brings back) |
| `short_text_box::text(&short, placeholder, on_action)` | the field, as libcosmic's `text_input` |
| `short_text_box::search(&short, placeholder, on_action)` | as `search_input`; `.on_clear(message)` shows its clear button, and the app clears the box with `replace` |
| `short_text_box::secure(&password, placeholder, on_action, hidden, on_toggle)` | as `secure_input`, over a `PasswordBox`, with its show-or-hide button when `on_toggle` is given; the other kinds take only a `ShortTextBox`, so a password cannot be shown plain nor a plain box keep a password's history |
| `short_text_box::inline(..)`, `short_text_box::editable(.., editing, on_toggle_edit)` | as `inline_input` and `editable_input` |
| `.on_submit(m)`, `.on_clear(m)`, `.on_focus(m)`, `.on_unfocus(m)`, `.on_tab(m)`, `.on_paste(f)`, `.id`, `.width`, `.padding`, `.size`, `.font`, `.label`, `.helper_text`, `.error`, `.select_on_focus`, `.leading_icon`, `.trailing_icon` | on the field, passed on to libcosmic's input with the app's own messages; `.on_paste` is told after the box's own edit |
| `.behind(window_behind)` | the field's window is behind others: its focused border is the rest colour |

The field is `text_menu(field, &words, style).history(TextMenuHistory::of(&short, on_action))` for its menu with Undo · Redo, as a text box's editor is; a password's has no history to offer.

**Shared with the text box, one copy each:** the history and its bounds, the rules that make its steps (a run of typing ended by moving the caret, by a space after a word, by changing between typing and deleting; a paste, a cut, a word deleted and the app's change a step each; typing over a selection one step with the typing after it), the app's part of a step (rule 19b), everything `Undoable` offers, `standard_keys()` and its key matching, the text menu's Undo · Redo (`TextMenuHistory::of` takes either box), and the kept selection's look and drawing.

**Different, as one-line fields are:**

- The app keeps the value, as with `text_input`. The caret is the widget's, so each `Edit` the field reports says where the caret was and is, and the key that made it; an Undo puts back the caret and selection by asking the field, which does it when it is next built (the app returns no task).
- Enter submits; there are no new lines.
- Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y are taken only while the field is focused; otherwise the key goes on to the app (Marginalium's Ctrl+Z undoes its notes). A password field takes them and does nothing.
- Ctrl+Backspace and Ctrl+Delete are libcosmic's own, by words (to the start or end in a password), so they are not written again here; Shift+click, which `text_input` lacks, is added.
- libcosmic's input lets its selection go on Esc and Tab and keeps it when a press elsewhere takes the focus (a menu opened): that kept selection is the one drawn dimmed. It is drawn from the input's state as it was just before that press; if the value changes or a password is shown or hidden meanwhile, the kept selection is no longer drawn. An input in its error look keeps it, with the kept colours.

**The kinds** are libcosmic's own constructors, one function each, giving one widget, `Field`: the look of each stays libcosmic's, and everything else is the same code. What an input method commits is typing, part of the run going on.

## Depending on it

```toml
household-widgets = { git = "https://github.com/ThomasAndJuliet/household-widgets.git" }
```

The crate depends on libcosmic as an app does, by upstream's URL with no rev:

```toml
libcosmic = { git = "https://github.com/pop-os/libcosmic.git", default-features = false }
```

Every household app has `[patch."https://github.com/pop-os/libcosmic.git"]` pointing at the household fork at the revision it pins. A patch replaces every dependency on that URL, so this crate is built with the app's own libcosmic: there is one libcosmic in the app's lock, selected by the app at a compatible fork revision. Naming the fork's URL here would give an app two libcosmics, and its `Element` would not be this crate's `Element`. The same goes for `household-tones`: this crate and the app must spell its URL the same way, as above, so that the app's lock has one.

This repository's own `Cargo.toml` has a `[patch]` for its tests, Clippy and CI. Apps using `TextMenu` must pin a compatible household libcosmic revision providing the native text target and session interfaces. This crate is tested against `cb889ef4bbfccd45739d5fa6f9140fd0e53df9fa`, which includes the native editing, popup and named-selection fixes; older fork revisions lack these interfaces or fixes. Cargo reads `[patch]` only from the workspace being built, so each app chooses its own compatible fork revision.

A Flatpak app needs the crate in its `cargo-sources.json`.

## Adding a widget

1. Move the best of the apps' copies here, one module per widget, and keep its behaviour unless a rule says otherwise. Work out what every app's call sites use first, so that each changes by a line.
2. Take colours from `household-tones`. If the maths is missing, add it there.
3. Add tests that draw the real widget with iced's headless tiny-skia renderer and read the pixels, as `src/switch_tests.rs` does: the expected look comes from the rule's own values, or from libcosmic's widget drawn beside it.
4. Name the rule and Juliet's words in the module's documentation.
5. Build against the supported libcosmic revisions. `TextMenu` requires the native text target and session interfaces; older revisions cannot build it. `Widget` and the theme can change between revisions.

## Checks and CI

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --locked
cargo test --all-features --locked
```

`.github/workflows/ci.yml` runs those four commands. Pushes to this repository, same-repository pull requests and manual runs use the household's self-hosted runner (or a GitHub-hosted one when the `CI_RUNNER` variable is `hosted`). Pull requests from forks run on a GitHub-hosted runner with a read-only token and never on the self-hosted runner. No secret is used.

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
