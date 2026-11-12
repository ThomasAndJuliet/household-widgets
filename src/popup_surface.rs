// SPDX-License-Identifier: GPL-3.0-only

//! Shared surface for menus and informational hover cards.
//! Matches Terminal’s existing tab-preview card.
const SHADOW_BLUR_RADIUS: f32 = 8.0;
const SHADOW_OFFSET_Y: f32 = 2.0;

/// Transparent space reserved by fit negotiation for the shared shadow.
pub fn shadow_padding() -> cosmic::iced::Padding {
    cosmic::iced::Padding::new(SHADOW_BLUR_RADIUS)
        .top(SHADOW_BLUR_RADIUS - SHADOW_OFFSET_Y)
        .bottom(SHADOW_BLUR_RADIUS + SHADOW_OFFSET_Y)
}

/// Include the shared surface's shadow in its complete outer layout box.
/// The content already owns its card, border and internal padding.
pub fn reserve_shadow<'a, Message: 'a>(
    content: crate::layout::Child<'a, Message>,
) -> crate::layout::Child<'a, Message> {
    let padding = shadow_padding();
    crate::layout::framed_child(content, padding, move |content| {
        cosmic::widget::container(content).padding(padding).into()
    })
}

pub fn style() -> cosmic::theme::Container<'static> {
    cosmic::theme::Container::custom(|theme| {
        let cosmic = theme.cosmic();
        let on: cosmic::iced::Color = cosmic.on_bg_color().into();
        cosmic::iced::widget::container::Style {
            icon_color: Some(cosmic.background(theme.transparent).component.on.into()),
            text_color: Some(cosmic.background(theme.transparent).component.on.into()),
            background: Some(cosmic::iced::Background::Color(
                cosmic.bg_component_color().into(),
            )),
            border: cosmic::iced::Border {
                color: cosmic::iced::Color { a: 0.12, ..on },
                width: 1.0,
                radius: cosmic.corner_radii.radius_s.into(),
            },
            shadow: cosmic::iced::Shadow {
                color: cosmic.shade.into(),
                offset: cosmic::iced::Vector::new(0.0, SHADOW_OFFSET_Y),
                blur_radius: SHADOW_BLUR_RADIUS,
            },
            snap: true,
        }
    })
}
