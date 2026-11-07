// SPDX-License-Identifier: GPL-3.0-only

//! Settings control alignment over the first label and caption lines.

use cosmic::Element;

pub const LEVEL_CAP: usize = 2;
const BODY_LINE: f32 = 21.0;
const CAPTION_LINE: f32 = 17.0;

pub fn first_lines(
    lines: usize,
    label: f32,
    gap: f32,
    description: Option<f32>,
    body_line: f32,
) -> f32 {
    let line = [BODY_LINE, body_line]
        .into_iter()
        .min_by(|a, b| {
            let off = |l: f32| (label / l - (label / l).round()).abs();
            off(*a).total_cmp(&off(*b))
        })
        .unwrap_or(BODY_LINE);
    let label_lines = (label / line).round().max(1.0) as usize;
    match description {
        Some(description) if label_lines < lines => {
            label + gap + ((lines - label_lines) as f32 * CAPTION_LINE).min(description)
        }
        _ => label.min(lines as f32 * line),
    }
}

/// A settings row with its control (the last child) moved to the middle
/// of the row's first [`LEVEL_CAP`] lines of text, or of the control's own
/// height where that is more. The text is the child before the control,
/// or the first child ([`Level::text_first`]) where a preview sits
/// between them. libcosmic's row centres the control on the whole row, so
/// it drifts down as the description wraps; and its flex row, for a bar,
/// asks for `justify_items`, which a flex row ignores, so the bar sat at
/// the row's top, nearer the divider above than the row's padding. A
/// control the flex row has put under the text is left where it is. The
/// control's band starts at the row's top and is never shorter than the
/// control, so it keeps the row's padding from the dividers
pub struct Level<'a, M> {
    content: Element<'a, M>,
    text_first: bool,
    body_line: fn() -> f32,
}

impl<'a, M> Level<'a, M> {
    pub fn new(content: impl Into<Element<'a, M>>, body_line: fn() -> f32) -> Self {
        Self {
            content: content.into(),
            text_first: false,
            body_line,
        }
    }

    /// The row's text is its first child, not the one before its control
    pub fn text_first(mut self) -> Self {
        self.text_first = true;
        self
    }
}

impl<'a, M: 'a> From<Level<'a, M>> for Element<'a, M> {
    fn from(level: Level<'a, M>) -> Self {
        Element::new(level)
    }
}

mod widget_impl {
    use cosmic::Renderer;
    use cosmic::iced::advanced::layout::{self, Layout};
    use cosmic::iced::advanced::renderer;
    use cosmic::iced::advanced::widget::{Operation, Tree, Widget};
    use cosmic::iced::advanced::{Clipboard, Shell, mouse, overlay};
    use cosmic::iced::{Event, Length, Point, Rectangle, Size, Vector};

    use super::{LEVEL_CAP, Level, first_lines};

    impl<M> Widget<M, cosmic::Theme, Renderer> for Level<'_, M> {
        forward_widget!(M, cosmic::Theme, Renderer; children, diff, size, operate, update, draw, mouse_interaction, overlay, drag_destinations);

        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &Renderer,
            limits: &layout::Limits,
        ) -> layout::Node {
            let node = self
                .content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, limits);
            let mut children = node.children().to_vec();
            let n = children.len();
            if n < 2 {
                return node;
            }
            let at = if self.text_first { 0 } else { n - 2 };
            let text = children[at].bounds();
            let control = children[n - 1].bounds();
            // Under the text, in a flex row too narrow for both
            if control.y >= text.y + text.height {
                return node;
            }
            // The label and description, or a label alone
            let first = match children[at].children() {
                [label, description] => {
                    let (label, description) = (label.bounds(), description.bounds());
                    let gap = (description.y - (label.y + label.height)).max(0.0);
                    first_lines(
                        LEVEL_CAP,
                        label.height,
                        gap,
                        Some(description.height),
                        (self.body_line)(),
                    )
                }
                _ => first_lines(LEVEL_CAP, text.height, 0.0, None, (self.body_line)()),
            };
            let band = first.min(text.height).max(control.height);
            children[n - 1].move_to_mut(Point::new(control.x, (band - control.height) / 2.0));
            let height = children
                .iter()
                .map(|c| c.bounds().y + c.bounds().height)
                .fold(0.0, f32::max);
            layout::Node::with_children(Size::new(node.size().width, height), children)
                .move_to(node.bounds().position())
        }
    }
}
