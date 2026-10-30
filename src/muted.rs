// SPDX-License-Identifier: GPL-3.0-only

//! The muted accent of household rule 6, for widgets whose look an app
//! cannot set: they are drawn with a theme whose accent is the muted one.
//! From Cerea's `ui.rs`.

use std::cell::RefCell;
use std::sync::Arc;

use cosmic::cosmic_theme::{Theme as CosmicTheme, palette::Srgba};
use cosmic::iced::Color;
use cosmic::theme::ThemeType;
use cosmic::{Element, Theme};
use household_tones::{Srgb, muted_accent, on_muted};

/// How many themes [`muted_theme`] keeps: one of each mode of each accent an
/// app is drawn with, which is two unless the accent is changing
pub(crate) const MAX_THEMES: usize = 16;

thread_local! {
    /// Each theme drawn with, and its muted form
    static THEMES: RefCell<Vec<(CosmicTheme, Arc<CosmicTheme>)>> = const { RefCell::new(Vec::new()) };
}

/// The muted accent of `theme`'s accent (household rule 6), and the text and
/// icon colour on it. The accent's alpha is kept.
pub fn muted_colours(theme: &Theme) -> (Color, Color) {
    let c = theme.cosmic();
    let accent = c.accent_color();
    let muted = muted_accent(Srgb::new(accent.red, accent.green, accent.blue), c.is_dark);
    let on = on_muted(muted);
    let colour = |c: Srgb| Color {
        r: c.r,
        g: c.g,
        b: c.b,
        a: accent.alpha,
    };
    (colour(muted), colour(on))
}

/// `theme` with its accent made the muted accent. The changed colours are
/// kept per theme, as a theme is large and this runs on every draw. A theme
/// is its whole self, not only its accent: the rest of what is drawn, such
/// as a switch's corners, must follow the system's.
pub(crate) fn muted_theme(theme: &Theme) -> Theme {
    let from = theme.cosmic();
    let made = THEMES.with(|themes| {
        let mut themes = themes.borrow_mut();
        if let Some((_, made)) = themes.iter().find(|(kept, _)| kept == from) {
            return made.clone();
        }
        let (muted, on) = muted_colours(theme);
        let to_srgba = |c: Color| Srgba::new(c.r, c.g, c.b, c.a);
        let mut changed = from.clone();
        changed.accent.base = to_srgba(muted);
        changed.accent.on = to_srgba(on);
        let made = Arc::new(changed);
        if themes.len() >= MAX_THEMES {
            themes.clear();
        }
        themes.push((from.clone(), made.clone()));
        made
    });
    Theme {
        theme_type: ThemeType::Custom(made),
        ..theme.clone()
    }
}

/// How many themes are kept on this thread
#[cfg(test)]
pub(crate) fn kept_themes() -> usize {
    THEMES.with(|themes| themes.borrow().len())
}

/// `content` drawn with the muted accent in place of the accent, for a
/// libcosmic widget that fills with the theme's accent and is not a plain
/// switch: a switch with a label, say,
/// `muted(widget::toggler(on).label(label).spacing(8).on_toggle(message))`.
/// Layout, events and everything else are `content`'s own. An overlay it
/// opens is drawn by the window, in the window's theme.
pub fn muted<'a, M: 'static>(content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    Element::new(Muted {
        content: content.into(),
    })
}

struct Muted<'a, M> {
    content: Element<'a, M>,
}

mod widget {
    use cosmic::iced::advanced::clipboard::DndDestinationRectangles;
    use cosmic::iced::advanced::layout::{self, Layout};
    use cosmic::iced::advanced::renderer;
    use cosmic::iced::advanced::widget::{Operation, Tree, Widget};
    use cosmic::iced::advanced::{Clipboard, Shell, mouse, overlay};
    use cosmic::iced::{Event, Length, Rectangle, Size, Vector};
    use cosmic::{Renderer, Theme};

    use super::{Muted, muted_theme};

    impl<M> Widget<M, Theme, Renderer> for Muted<'_, M> {
        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.content)]
        }
        fn diff(&mut self, tree: &mut Tree) {
            tree.diff_children(std::slice::from_mut(&mut self.content));
        }
        fn size(&self) -> Size<Length> {
            self.content.as_widget().size()
        }
        fn size_hint(&self) -> Size<Length> {
            self.content.as_widget().size_hint()
        }
        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &Renderer,
            limits: &layout::Limits,
        ) -> layout::Node {
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, limits)
        }
        fn operate(
            &mut self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            operation: &mut dyn Operation,
        ) {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                operation,
            );
        }
        fn update(
            &mut self,
            tree: &mut Tree,
            event: &Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &Renderer,
            clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, M>,
            viewport: &Rectangle,
        ) {
            self.content.as_widget_mut().update(
                &mut tree.children[0],
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            let muted = muted_theme(theme);
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                &muted,
                style,
                layout,
                cursor,
                viewport,
            );
        }
        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &Renderer,
        ) -> mouse::Interaction {
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                layout,
                cursor,
                viewport,
                renderer,
            )
        }
        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'b>,
            renderer: &Renderer,
            viewport: &Rectangle,
            translation: Vector,
        ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
            )
        }
        fn drag_destinations(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            rectangles: &mut DndDestinationRectangles,
        ) {
            self.content.as_widget().drag_destinations(
                &tree.children[0],
                layout,
                renderer,
                rectangles,
            );
        }
    }
}
