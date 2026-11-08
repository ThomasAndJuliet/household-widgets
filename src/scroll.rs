// SPDX-License-Identifier: GPL-3.0-only

//! Scrolling controls reserve their own lane instead of covering content.

use cosmic::iced::advanced::widget::{Operation, Tree};
use cosmic::iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use cosmic::iced::widget::scrollable::{Direction, Scrollbar};
use cosmic::iced::{Event, Length, Rectangle, Size, Vector};
use cosmic::{Element, Renderer, Theme, widget};

pub(crate) const SCROLLBAR_WIDTH: f32 = 8.0;
pub(crate) const SCROLLBAR_GAP: f32 = 2.0;

/// A horizontal viewport with an embedded scrollbar below its content.
/// The outer height includes the shared scrollbar lane, even without overflow.
pub fn horizontal_scroll<'a, Message: 'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    let content = widget::scrollable(content)
        .direction(Direction::Horizontal(
            Scrollbar::new()
                .width(SCROLLBAR_WIDTH)
                .scroller_width(SCROLLBAR_WIDTH)
                .spacing(SCROLLBAR_GAP),
        ))
        .width(Length::Fill)
        .height(Length::Shrink)
        .into();
    Element::new(HorizontalScroll { content })
}

struct HorizontalScroll<'a, Message> {
    content: Element<'a, Message>,
}

impl<Message> Widget<Message, Theme, Renderer> for HorizontalScroll<'_, Message> {
    forward_widget!(Message, Theme, Renderer; children, diff, draw, update, operate, mouse_interaction, overlay, drag_destinations);

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let maximum = Size::new(limits.max().width, f32::INFINITY);
        // Keep the non-scrolling axis shrink-compressed. Otherwise native
        // fixed-height content can make the scrollable enclose only the
        // control's height, omitting its embedded lane from the outer box.
        let node = self.content.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &layout::Limits::with_compression(
                Size::ZERO,
                maximum,
                Size::new(maximum.width.is_infinite(), true),
            ),
        );
        // Native layout reserves the embedded lane when content overflows,
        // but omits it when it fits. Keep the same useful outer height in both
        // states, without adding a second lane to the overflowing case.
        let content_bottom = node
            .children()
            .first()
            .map_or(0.0, |content| content.bounds().y + content.size().height);
        let height = node
            .size()
            .height
            .max(content_bottom + SCROLLBAR_WIDTH + SCROLLBAR_GAP);
        layout::Node::with_children(
            Size::new(node.size().width, height),
            node.children().to_vec(),
        )
    }

    #[cfg(feature = "a11y")]
    fn a11y_nodes(
        &self,
        layout: Layout<'_>,
        tree: &Tree,
        cursor: mouse::Cursor,
    ) -> iced_accessibility::A11yTree {
        self.content
            .as_widget()
            .a11y_nodes(layout, &tree.children[0], cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic::iced::advanced::{layout, renderer::Headless, widget::Tree};
    use cosmic::iced::{Font, Pixels, Size};

    #[test]
    fn horizontal_scroll_keeps_the_bar_below_native_controls_at_any_content_height() {
        let renderer = cosmic::iced::futures::executor::block_on(
            <cosmic::Renderer as Headless>::new(Font::default(), Pixels(14.0), Some("tiny-skia")),
        )
        .expect("headless renderer");
        for height in [32.0, 48.0] {
            for width in [100.0, 600.0] {
                let mut view: Element<'_, ()> =
                    horizontal_scroll(widget::Space::new().width(500.0).height(height).into());
                let mut tree = Tree::new(view.as_widget());
                tree.diff(view.as_widget_mut());
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, Size::new(width, f32::INFINITY)),
                );
                assert_eq!(
                    node.size().height,
                    height + SCROLLBAR_WIDTH + SCROLLBAR_GAP,
                    "content {width}x{height}"
                );
                let content = node.children()[0].bounds();
                assert_eq!(content.height, height);
                assert!(content.y + content.height + SCROLLBAR_WIDTH <= node.size().height);
            }
        }
    }
}
