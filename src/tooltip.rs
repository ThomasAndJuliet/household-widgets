// SPDX-License-Identifier: GPL-3.0-only

//! The household tooltip bubble and its stable widget shape.

use cosmic::iced::Padding;
use cosmic::widget::{self, tooltip::Position};
use cosmic::{Element, theme};

pub const TIP_WIDTH: f32 = 320.0;

pub fn tip_padding() -> Padding {
    let space = theme::active().cosmic().spacing;
    Padding::from([space.space_xs, space.space_s])
}

pub fn tip_bubble<'a, M: 'a>(
    tip: impl Into<Element<'a, M>>,
) -> widget::Container<'a, M, cosmic::Theme> {
    widget::container(tip).padding(tip_padding())
}

pub fn tooltip_with<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    tip: impl Into<Element<'a, M>>,
    position: Position,
) -> Element<'a, M> {
    let padding = tip_padding();
    let bubble = tip_bubble(tip).max_width(TIP_WIDTH + padding.left + padding.right);
    widget::tooltip(content, bubble, position)
        .padding(0.0)
        .into()
}

pub fn tooltip_unless<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    tip: impl Into<Element<'a, M>>,
    position: Position,
    hidden: bool,
) -> Element<'a, M> {
    if hidden {
        widget::tooltip(
            content,
            widget::Space::new().width(0.0).height(0.0),
            position,
        )
        .padding(0.0)
        .into()
    } else {
        tooltip_with(content, tip, position)
    }
}

pub fn link_tooltip<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    tip: Option<impl Into<Element<'a, M>>>,
) -> Element<'a, M> {
    match tip {
        Some(tip) => {
            let gap = theme::active().cosmic().spacing.space_xs;
            let bubble = tip_bubble(tip).class(theme::Container::Tooltip);
            let tip = widget::column::with_children(vec![
                bubble.into(),
                widget::Space::new().height(gap).into(),
            ]);
            widget::tooltip(content, tip, Position::FollowCursor)
                .padding(0.0)
                .class(theme::Container::Transparent)
                .into()
        }
        None => widget::tooltip(content, widget::Space::new(), Position::FollowCursor)
            .padding(0.0)
            .class(theme::Container::Transparent)
            .into(),
    }
}
