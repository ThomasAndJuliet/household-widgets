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
    on_press: Option<M>,
    leading: Option<widget::icon::Handle>,
    trailing: Option<widget::icon::Handle>,
    width: Length,
    class: theme::Button,
    noop: M,
    _life: std::marker::PhantomData<&'a ()>,
}

impl<'a, M: Clone> Tertiary<'a, M> {
    pub fn new(label: impl Into<String>, class: theme::Button, noop: M) -> Self {
        Self {
            label: label.into(),
            on_press: None,
            leading: None,
            trailing: None,
            width: Length::Shrink,
            class,
            noop,
            _life: std::marker::PhantomData,
        }
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

impl<'a, M: Clone + 'a> From<Tertiary<'a, M>> for Element<'a, M> {
    fn from(tertiary: Tertiary<'a, M>) -> Self {
        let space = theme::active().cosmic().spacing;
        let noop = tertiary.noop;
        let label = cosmic::iced::widget::rich_text([cosmic::iced::widget::span(tertiary.label)
            .underline(true)
            .font(cosmic::font::bold())])
        .size(14.0)
        .line_height(LineHeight::Absolute(20.0.into()))
        .on_link_click(move |()| noop.clone());
        let mut parts: Vec<Element<'a, M>> = Vec::with_capacity(3);
        parts.extend(
            tertiary
                .leading
                .map(|icon| widget::icon::icon(icon).size(16).into()),
        );
        parts.push(label.into());
        parts.extend(
            tertiary
                .trailing
                .map(|icon| widget::icon::icon(icon).size(16).into()),
        );
        let content = widget::row::with_children(parts)
            .spacing(space.space_xxs)
            .align_y(Alignment::Center)
            .height(Length::Fill);
        button::custom(content)
            .class(tertiary.class)
            .padding([space.space_xxxs, space.space_xxs])
            .height(Length::Fixed(f32::from(space.space_l)))
            .width(tertiary.width)
            .on_press_maybe(tertiary.on_press)
            .into()
    }
}
