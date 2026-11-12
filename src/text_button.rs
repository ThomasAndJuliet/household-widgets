// SPDX-License-Identifier: GPL-3.0-only

//! Shared text-button content and layout. Each application supplies its palette.

use cosmic::iced::advanced::text::LineHeight;
use cosmic::iced::{Alignment, Length, Padding};
use cosmic::widget::{self, button};
use cosmic::{Element, theme};

pub const BUTTON_HEIGHT: f32 = 32.0;
const BUTTON_TEXT: f32 = 14.0;
const BUTTON_LINE: f32 = 20.0;

pub fn button_padding() -> Padding {
    let space = theme::active().cosmic().spacing;
    let v = (BUTTON_HEIGHT - BUTTON_LINE) / 2.0;
    let side = f32::from(space.space_s);
    Padding {
        top: v,
        bottom: v,
        left: side,
        right: side,
    }
}

pub struct TextButton<'a, M> {
    label: String,
    on_press: Option<M>,
    leading: Option<widget::icon::Handle>,
    trailing: Option<widget::icon::Handle>,
    width: Length,
    class: theme::Button,
    _life: std::marker::PhantomData<&'a ()>,
}

impl<'a, M> TextButton<'a, M> {
    pub fn new(label: impl Into<String>, class: theme::Button) -> Self {
        Self {
            label: label.into(),
            on_press: None,
            leading: None,
            trailing: None,
            width: Length::Shrink,
            class,
            _life: std::marker::PhantomData,
        }
    }

    pub fn class(mut self, class: theme::Button) -> Self {
        self.class = class;
        self
    }

    pub fn on_press(mut self, message: M) -> Self {
        self.on_press = Some(message);
        self
    }

    pub fn on_press_maybe(mut self, message: Option<M>) -> Self {
        self.on_press = message;
        self
    }

    pub fn leading_icon(mut self, icon: impl Into<widget::icon::Handle>) -> Self {
        self.leading = Some(icon.into());
        self
    }

    pub fn trailing_icon(mut self, icon: impl Into<widget::icon::Handle>) -> Self {
        self.trailing = Some(icon.into());
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
}

impl<'a, M: Clone + 'a> From<TextButton<'a, M>> for Element<'a, M> {
    fn from(button: TextButton<'a, M>) -> Self {
        let space = theme::active().cosmic().spacing;
        let fixed = !matches!(button.width, Length::Shrink);
        let mut parts: Vec<Element<'a, M>> = Vec::with_capacity(3);
        parts.extend(
            button
                .leading
                .map(|icon| widget::icon::icon(icon).size(16).into()),
        );
        parts.push(
            widget::text(button.label)
                .size(BUTTON_TEXT)
                .line_height(LineHeight::Absolute(BUTTON_LINE.into()))
                .font(cosmic::font::semibold())
                .wrapping(cosmic::iced::widget::text::Wrapping::None)
                .into(),
        );
        parts.extend(
            button
                .trailing
                .map(|icon| widget::icon::icon(icon).size(16).into()),
        );
        let content = widget::row::with_children(parts)
            .spacing(space.space_xxs)
            .align_y(Alignment::Center);
        let row: Element<'a, M> = if fixed {
            widget::row::with_children(vec![
                widget::Space::new().width(Length::Fill).into(),
                content.into(),
                widget::Space::new().width(Length::Fill).into(),
            ])
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into()
        } else {
            content.into()
        };
        button::custom(row)
            .class(button.class)
            .padding(button_padding())
            .width(button.width)
            .on_press_maybe(button.on_press)
            .into()
    }
}

pub struct Tertiary<'a, M> {
    label: String,
    id: Option<widget::Id>,
    on_press: Option<M>,
    leading: Option<widget::icon::Handle>,
    trailing: Option<widget::icon::Handle>,
    leading_element: Option<Element<'a, M>>,
    height: Option<f32>,
    icon_only: bool,
    width: Length,
    class: theme::Button,
    noop: M,
}

impl<'a, M: Clone> Tertiary<'a, M> {
    pub fn new(label: impl Into<String>, class: theme::Button, noop: M) -> Self {
        Self {
            label: label.into(),
            id: None,
            on_press: None,
            leading: None,
            trailing: None,
            leading_element: None,
            height: None,
            icon_only: false,
            width: Length::Shrink,
            class,
            noop,
        }
    }

    /// Give the native control a stable identity for focus navigation.
    pub fn id(mut self, id: widget::Id) -> Self {
        self.id = Some(id);
        self
    }

    pub fn on_press(mut self, message: M) -> Self {
        self.on_press = Some(message);
        self
    }

    pub fn on_press_maybe(mut self, message: Option<M>) -> Self {
        self.on_press = message;
        self
    }

    pub fn leading_icon(mut self, icon: impl Into<widget::icon::Handle>) -> Self {
        self.leading = Some(icon.into());
        self
    }

    /// Use a custom leading element, in preference to a leading icon handle.
    pub fn leading_element(mut self, element: Element<'a, M>) -> Self {
        self.leading_element = Some(element);
        self
    }

    /// Set the target height. Icon-only targets use the same width.
    ///
    /// Panics if the height is not finite and positive.
    pub fn height(mut self, height: f32) -> Self {
        assert!(
            height.is_finite() && height > 0.0,
            "height must be finite and positive"
        );
        self.height = Some(height);
        self
    }

    /// Show only the leading element or icon, retaining the label for accessibility.
    pub fn icon_only(mut self, icon_only: bool) -> Self {
        self.icon_only = icon_only;
        self
    }

    pub fn trailing_icon(mut self, icon: impl Into<widget::icon::Handle>) -> Self {
        self.trailing = Some(icon.into());
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
}

impl<'a, M: Clone + 'a> From<Tertiary<'a, M>> for Element<'a, M> {
    fn from(tertiary: Tertiary<'a, M>) -> Self {
        let space = theme::active().cosmic().spacing;
        let height = tertiary.height.unwrap_or(f32::from(space.space_l));
        #[cfg(feature = "a11y")]
        let accessible_name = tertiary.label.clone();
        let mut parts: Vec<Element<'a, M>> = Vec::with_capacity(3);
        parts.extend(tertiary.leading_element.or_else(|| {
            tertiary
                .leading
                .map(|icon| widget::icon::icon(icon).size(16).into())
        }));
        if !tertiary.icon_only {
            let noop = tertiary.noop;
            let label =
                cosmic::iced::widget::rich_text([cosmic::iced::widget::span(tertiary.label)
                    .underline(true)
                    .font(cosmic::font::bold())])
                .size(14.0)
                .line_height(LineHeight::Absolute(20.0.into()))
                .on_link_click(move |()| noop.clone());
            parts.push(label.into());
            parts.extend(
                tertiary
                    .trailing
                    .map(|icon| widget::icon::icon(icon).size(16).into()),
            );
        }
        let content = widget::row::with_children(parts)
            .spacing(space.space_xxs)
            .align_y(Alignment::Center)
            .height(Length::Fill);
        let button = button::custom(content)
            .class(tertiary.class)
            .padding([space.space_xxxs, space.space_xxs])
            .height(Length::Fixed(height))
            .width(if tertiary.icon_only {
                Length::Fixed(height)
            } else {
                tertiary.width
            })
            .on_press_maybe(tertiary.on_press);
        let button = match tertiary.id {
            Some(id) => button.id(id),
            None => button,
        };
        let button = if tertiary.icon_only {
            button.align_x(Alignment::Center)
        } else {
            button
        };
        #[cfg(feature = "a11y")]
        let button = button.name(accessible_name);
        button.into()
    }
}

#[cfg(test)]
#[path = "text_button_tests.rs"]
mod tests;
