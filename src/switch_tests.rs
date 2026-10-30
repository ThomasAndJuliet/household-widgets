// SPDX-License-Identifier: GPL-3.0-only

//! Rule 6: "a switch that is on uses the muted accent": "yes use the muted
//! accent for them all" (Juliet, 30 Oct), and nothing else about libcosmic's
//! switch changes.
//!
//! The real widgets are drawn with iced's headless tiny-skia renderer and
//! the pixels read, as household-test's painter does. Each expected look is
//! libcosmic's own toggler, drawn beside the switch in the same way.

use std::sync::Arc;

use cosmic::cosmic_theme::{ThemeBuilder, palette::Srgb as Palette};
use cosmic::iced::advanced::clipboard;
use cosmic::iced::advanced::renderer::{Headless, Style};
use cosmic::iced::runtime::user_interface::{Cache, UserInterface};
use cosmic::iced::{Color, Event, Font, Length, Pixels, Point, Size, mouse};
use cosmic::widget::settings;
use cosmic::{Element, Theme, widget};
use household_tones::{Srgb, muted_accent, on_muted};

use crate::muted::{MAX_THEMES, kept_themes, muted_theme};
use crate::{SwitchItem, muted, muted_colours, switch, switch_maybe};

type Message = bool;
type Rgb = [u8; 3];

const SIZE: Size = Size::new(240.0, 48.0);
/// The switch is at the window's top left; 48 x 24
const ON_FILL: Point = Point::new(10.0, 12.0);
const OFF_FILL: Point = Point::new(38.0, 12.0);

/// What was drawn, and the messages published on the way
struct Shot {
    rgba: Vec<u8>,
    messages: Vec<Message>,
}

impl Shot {
    fn at(&self, at: Point) -> Rgb {
        let i = (at.y as usize * SIZE.width as usize + at.x as usize) * 4;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2]]
    }
}

/// `build`'s widget at the window's top left in `theme` on black, with the
/// pointer at `pointer`, after `events`
fn draw(
    build: impl Fn() -> Element<'static, Message>,
    theme: &Theme,
    pointer: Option<Point>,
    events: &[Event],
) -> Shot {
    let mut renderer = cosmic::iced::futures::executor::block_on(
        <cosmic::Renderer as Headless>::new(Font::default(), Pixels(14.0), Some("tiny-skia")),
    )
    .expect("a headless renderer");
    let cursor = pointer.map_or(mouse::Cursor::Unavailable, mouse::Cursor::Available);
    let mut messages = Vec::new();
    let mut cache = Cache::default();
    // Drawn twice, the first not kept, as household-test's painter does
    for pass in 0..2 {
        let element = widget::container(build())
            .width(Length::Fill)
            .height(Length::Fill);
        let mut ui = UserInterface::build(element, SIZE, cache, &mut renderer);
        if pass == 0 {
            for event in events {
                let _ = ui.update(
                    std::slice::from_ref(event),
                    cursor,
                    &mut renderer,
                    &mut clipboard::Null,
                    &mut messages,
                );
            }
        }
        let style = Style {
            text_color: Color::WHITE,
            ..Style::default()
        };
        ui.draw(&mut renderer, theme, &style, cursor);
        cache = ui.into_cache();
    }
    let rgba = renderer.screenshot(
        cosmic::iced::Size::new(SIZE.width as u32, SIZE.height as u32),
        1.0,
        Color::BLACK,
    );
    Shot { rgba, messages }
}

fn plain(on: bool) -> Element<'static, Message> {
    widget::toggler(on).on_toggle(|on| on).into()
}

fn plain_disabled(on: bool) -> Element<'static, Message> {
    widget::toggler(on).into()
}

/// A click at `at`: the pointer there, pressed and released
fn click(at: Point) -> Vec<Event> {
    vec![
        Event::Mouse(mouse::Event::CursorMoved { position: at }),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ]
}

/// A custom theme, light or dark, with the accent `rgb`
fn theme_with(dark: bool, [r, g, b]: Rgb) -> Theme {
    let builder = if dark {
        ThemeBuilder::dark()
    } else {
        ThemeBuilder::light()
    };
    let accent = Palette::new(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    );
    Theme::custom(Arc::new(builder.accent(accent).build()))
}

/// Juliet's reference pairs for orange (`sheets/muted-toggles.png`): the
/// mode's accent, and its muted accent as she gave it
fn themes() -> [(Theme, Rgb, Srgb, bool); 2] {
    [
        (
            theme_with(false, [0x7f, 0x24, 0x00]),
            [0x7f, 0x24, 0x00],
            Srgb::hex(0x954b33),
            false,
        ),
        (
            theme_with(true, [0xff, 0x9f, 0x7d]),
            [0xff, 0x9f, 0x7d],
            Srgb::hex(0xd7876d),
            true,
        ),
    ]
}

fn distance(a: Rgb, b: Rgb) -> u8 {
    (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap()
}

fn rgb8(colour: Srgb) -> Rgb {
    colour.to_rgb8()
}

#[test]
fn an_on_switch_is_filled_with_the_muted_accent() {
    for (theme, accent, reference, dark) in themes() {
        let [r, g, b] = accent;
        let expected = muted_accent(Srgb::from_rgb8(r, g, b), dark);
        let shot = draw(|| switch(true, |on| on), &theme, None, &[]);
        let fill = shot.at(ON_FILL);
        assert!(
            distance(fill, rgb8(expected)) <= 1,
            "dark {dark}: {fill:?} is not household_tones' {:?}",
            rgb8(expected)
        );
        assert!(
            distance(fill, rgb8(reference)) <= 2,
            "dark {dark}: {fill:?} is not Juliet's {:?}",
            rgb8(reference)
        );
        // What libcosmic draws, which the switch must not be
        let libcosmic = draw(|| plain(true), &theme, None, &[]).at(ON_FILL);
        assert!(
            distance(libcosmic, accent) <= 1,
            "the reference: {libcosmic:?}"
        );
        assert!(distance(fill, accent) > 2);
    }
}

#[test]
fn muted_colours_are_household_tones() {
    for (theme, accent, _, dark) in themes() {
        let [r, g, b] = accent;
        let muted = muted_accent(Srgb::from_rgb8(r, g, b), dark);
        let (fill, on) = muted_colours(&theme);
        assert_eq!([fill.r, fill.g, fill.b], [muted.r, muted.g, muted.b]);
        let on_expected = on_muted(muted);
        assert_eq!(
            [on.r, on.g, on.b],
            [on_expected.r, on_expected.g, on_expected.b]
        );
        // The theme a switch is drawn with carries both
        let changed = muted_theme(&theme);
        let base = changed.cosmic().accent.base;
        let text = changed.cosmic().accent.on;
        assert_eq!(
            [base.red, base.green, base.blue],
            [muted.r, muted.g, muted.b]
        );
        assert_eq!(
            [text.red, text.green, text.blue],
            [on_expected.r, on_expected.g, on_expected.b]
        );
    }
}

#[test]
fn an_off_switch_is_libcosmics() {
    for (theme, ..) in themes() {
        let ours = draw(|| switch(false, |on| on), &theme, None, &[]);
        let libcosmic = draw(|| plain(false), &theme, None, &[]);
        assert!(ours.rgba == libcosmic.rgba, "the frames differ");
        assert_ne!(ours.at(OFF_FILL), [0, 0, 0], "nothing was drawn");
    }
}

#[test]
fn a_disabled_off_switch_is_libcosmics() {
    for (theme, ..) in themes() {
        let ours = draw(
            || switch_maybe(false, None::<fn(bool) -> Message>),
            &theme,
            None,
            &[],
        );
        let libcosmic = draw(|| plain_disabled(false), &theme, None, &[]);
        assert!(ours.rgba == libcosmic.rgba, "the frames differ");
    }
}

/// libcosmic's disabled look, on the muted fill: rule 6, "a consistently
/// faded, visibly muted treatment", "including ... toggles that are on"
#[test]
fn a_disabled_on_switch_is_libcosmics_faded_muted_fill() {
    for (theme, ..) in themes() {
        let ours = draw(
            || switch_maybe(true, None::<fn(bool) -> Message>),
            &theme,
            None,
            &[],
        );
        let reference = draw(|| plain_disabled(true), &muted_theme(&theme), None, &[]);
        assert!(ours.rgba == reference.rgba, "the frames differ");
        // At half alpha, over the black drawn on
        let enabled = draw(|| switch(true, |on| on), &theme, None, &[]).at(ON_FILL);
        let half = enabled.map(|c| (f32::from(c) / 2.0).round() as u8);
        let fill = ours.at(ON_FILL);
        assert!(
            distance(fill, half) <= 2,
            "{fill:?} is not half of {enabled:?}"
        );
    }
}

#[test]
fn a_hovered_switch_is_libcosmics_hover_on_the_muted_fill() {
    for (theme, ..) in themes() {
        let hover = Some(ON_FILL);
        let ours = draw(|| switch(true, |on| on), &theme, hover, &[]);
        let reference = draw(|| plain(true), &muted_theme(&theme), hover, &[]);
        assert!(ours.rgba == reference.rgba, "the frames differ");
        let rest = draw(|| switch(true, |on| on), &theme, None, &[]);
        assert_ne!(ours.at(ON_FILL), rest.at(ON_FILL), "no hover drawn");
    }
}

#[test]
fn a_click_publishes_the_new_state() {
    let (theme, ..) = &themes()[0];
    for on in [false, true] {
        let at = Some(OFF_FILL);
        let shot = draw(|| switch(on, |on| on), theme, at, &click(OFF_FILL));
        assert_eq!(shot.messages, [!on]);
        let shot = draw(
            || switch_maybe(on, Some(|on| on)),
            theme,
            at,
            &click(OFF_FILL),
        );
        assert_eq!(shot.messages, [!on]);
    }
}

#[test]
fn a_disabled_switch_publishes_nothing() {
    let (theme, ..) = &themes()[0];
    let shot = draw(
        || switch_maybe(true, None::<fn(bool) -> Message>),
        theme,
        Some(ON_FILL),
        &click(ON_FILL),
    );
    assert!(shot.messages.is_empty());
}

/// Marginalium's presenter console: `.label(..).spacing(..)`
#[test]
fn a_labelled_switch_keeps_its_layout_and_label() {
    let labelled = |on: bool| {
        widget::toggler(on)
            .label("Pointer".to_string())
            .spacing(8)
            .on_toggle(|on| on)
    };
    for (theme, accent, _, dark) in themes() {
        let [r, g, b] = accent;
        let expected = rgb8(muted_accent(Srgb::from_rgb8(r, g, b), dark));
        // Off: exactly libcosmic's, label and all
        let ours = draw(|| muted(labelled(false)), &theme, None, &[]);
        let libcosmic = draw(|| labelled(false).into(), &theme, None, &[]);
        assert!(ours.rgba == libcosmic.rgba, "the frames differ");
        // On: libcosmic's, with the muted fill
        let ours = draw(|| muted(labelled(true)), &theme, None, &[]);
        let reference = draw(|| labelled(true).into(), &muted_theme(&theme), None, &[]);
        assert!(ours.rgba == reference.rgba, "the frames differ");
        let fill = (0..240)
            .map(|x| Point::new(x as f32, 12.0))
            .filter(|&at| distance(ours.at(at), expected) <= 1)
            .count();
        assert!(fill > 10, "no muted fill in the labelled switch");
    }
}

/// `.toggler` and `.switch` on a settings item
#[test]
fn a_settings_row_flips_as_libcosmics() {
    let ours = |on: bool| -> Element<'static, Message> {
        widget::list_column()
            .add(settings::item::builder("Autosave").switch(on, |on| on))
            .into()
    };
    let libcosmic = |on: bool| -> Element<'static, Message> {
        widget::list_column()
            .add(settings::item::builder("Autosave").toggler(on, |on| on))
            .into()
    };
    let (theme, ..) = &themes()[0];
    let on_the_row = Point::new(40.0, 20.0);
    let on_the_control = Point::new(205.0, 20.0);
    for on in [false, true] {
        for at in [on_the_row, on_the_control] {
            let mine = draw(|| ours(on), theme, Some(at), &click(at));
            let theirs = draw(|| libcosmic(on), theme, Some(at), &click(at));
            assert_eq!(mine.messages, theirs.messages, "at {at:?}");
            assert_eq!(mine.messages, [!on], "at {at:?}");
        }
    }
}

#[test]
fn a_disabled_settings_row_publishes_nothing() {
    let (theme, ..) = &themes()[0];
    let shot = draw(
        || {
            widget::list_column()
                .add(
                    settings::item::builder("Autosave")
                        .switch_maybe(true, None::<fn(bool) -> Message>),
                )
                .into()
        },
        theme,
        Some(Point::new(40.0, 20.0)),
        &click(Point::new(40.0, 20.0)),
    );
    assert!(shot.messages.is_empty());
}

#[test]
fn the_kept_themes_are_bounded_and_reused() {
    let theme = theme_with(false, [1, 2, 3]);
    let _ = muted_theme(&theme);
    let kept = kept_themes();
    for _ in 0..100 {
        let _ = muted_theme(&theme);
    }
    assert_eq!(kept_themes(), kept, "a theme kept is used again");
    for i in 0..(3 * MAX_THEMES) as u8 {
        let _ = muted_theme(&theme_with(i % 2 == 0, [i, 40, 80]));
        assert!(kept_themes() <= MAX_THEMES, "{} themes kept", kept_themes());
    }
}

/// A theme that differs only in its mode, or only in something other than
/// its accent, is its own: the system's Style setting changes the corners
/// and leaves the accent
#[test]
fn a_kept_theme_follows_the_rest_of_the_system_theme() {
    let light = theme_with(false, [0x7f, 0x24, 0x00]);
    let dark = theme_with(true, [0x7f, 0x24, 0x00]);
    let mut square = light.cosmic().clone();
    square.corner_radii.radius_xl = [0.0; 4];
    let square = Theme::custom(Arc::new(square));

    assert!(!muted_theme(&light).cosmic().is_dark);
    assert!(muted_theme(&dark).cosmic().is_dark);
    let _ = draw(|| switch(false, |on| on), &light, None, &[]);
    let ours = draw(|| switch(false, |on| on), &square, None, &[]);
    let libcosmic = draw(|| plain(false), &square, None, &[]);
    assert!(ours.rgba == libcosmic.rgba, "the frames differ");
}

/// The layer a widget is drawn on survives, for `muted` around widgets that
/// look at it
#[test]
fn the_muted_theme_keeps_the_layer() {
    let (theme, ..) = &themes()[0];
    let mut layered = theme.clone();
    layered.layer = cosmic::cosmic_theme::Layer::Primary;
    assert_eq!(muted_theme(&layered).layer, layered.layer);
}
