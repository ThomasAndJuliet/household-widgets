// SPDX-License-Identifier: GPL-3.0-only

//! Composable building blocks for menu contents. State, including expansion and
//! whether an activation closes a popup, belongs to the application.

use cosmic::Element;
use cosmic::iced::widget::{self, tooltip::Position};
use cosmic::iced::{Alignment, Length};

/// A row's contour is part of its enclosing composition. The adjoining lower
/// corners follow the frame's inner contour, rather than adding another radius.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum RowContour {
    #[default]
    Rounded,
    OpenTitle,
    FrameBottom {
        inset: f32,
    },
}

impl RowContour {
    pub fn radius(self, theme: &cosmic::Theme) -> cosmic::iced::border::Radius {
        let [tl, tr, br, bl] = theme.cosmic().corner_radii.radius_s;
        match self {
            Self::Rounded => [tl, tr, br, bl].into(),
            Self::OpenTitle => [tl, tr, 0.0, 0.0].into(),
            Self::FrameBottom { inset } => {
                [tl, tr, (br - inset).max(0.0), (bl - inset).max(0.0)].into()
            }
        }
    }

    pub fn target_border(self, theme: &cosmic::Theme) -> cosmic::iced::Border {
        let mode = if theme.cosmic().is_dark {
            household_tones::Mode::Dark
        } else {
            household_tones::Mode::Light
        };
        let dark = household_tones::Role::Dark
            .tone(mode)
            .expect("dark household tone");
        cosmic::iced::Border {
            color: cosmic::iced::Color::from_rgb(dark.r, dark.g, dark.b),
            width: 1.0,
            radius: self.radius(theme),
        }
    }
}

/// Shared household row appearance, usable by arbitrary list compositions.
/// Label typography remains the caller's responsibility; selected and open
/// menu labels use its semibold body callback.
pub fn row_style(
    selected: bool,
    open: bool,
    contour: RowContour,
) -> cosmic::theme::style::iced::Button {
    cosmic::theme::style::iced::Button::Custom(Box::new(move |theme, status| {
        use widget::button::{Catalog, Status};
        let mut style = Catalog::style(theme, &cosmic::theme::style::iced::Button::Text, status);
        let cosmic = theme.cosmic();
        style.border.radius = contour.radius(theme);
        style.border_radius = contour.radius(theme);
        style.text_color = cosmic.on_bg_color().into();
        if status == Status::Disabled {
            style.background = selected.then(|| {
                let mut wash: cosmic::iced::Color = cosmic.icon_button.hover.into();
                wash.a *= 0.5;
                wash.into()
            });
            style.text_color.a *= 0.5;
        } else if open {
            let (fill, on) = crate::muted_colours(theme);
            let wash = match status {
                Status::Hovered => Some(cosmic.icon_button.hover),
                Status::Pressed => Some(cosmic.icon_button.pressed),
                _ => None,
            };
            style.background = Some(
                wash.map_or(fill, |wash| {
                    crate::kept::flat(cosmic::iced::Background::Color(wash.into()), fill)
                })
                .into(),
            );
            style.text_color = on;
        } else if selected {
            style.background = Some(if status == Status::Pressed {
                cosmic.icon_button.pressed.into()
            } else {
                cosmic.icon_button.hover.into()
            });
            style.text_color = cosmic.accent_text_color().into();
        } else {
            style.background = match status {
                Status::Hovered => Some(cosmic.icon_button.hover.into()),
                Status::Pressed => Some(cosmic.icon_button.pressed.into()),
                _ => None,
            };
        }
        style.icon_color = Some(style.text_color);
        style
    }))
}

/// Give a row its native target and its family's dark-grey contour. Disabled
/// rows use the same target wrapper and omit `on_activate`.
pub fn row_target<'a, M>(
    id: impl Into<cosmic::iced::advanced::widget::Id>,
    content: impl Into<Element<'a, M>>,
    contour: RowContour,
) -> crate::popup::Focusable<'a, M> {
    crate::popup::focus_target(id, content).target_border(move |theme| contour.target_border(theme))
}

/// The accepted inline disclosure frame: one outer contour shared by its
/// title, with the normal focused text-box border around its contents.
pub fn inline_group<'a, M: 'a>(title: Element<'a, M>, contents: Element<'a, M>) -> Element<'a, M> {
    let edge = crate::look::Look::normal()
        .border(&cosmic::theme::active(), crate::look::State::Focused)
        .width;
    let contents = widget::container(contents)
        .width(Length::Fill)
        .padding(cosmic::iced::Padding {
            top: 0.0,
            right: edge,
            bottom: edge,
            left: edge,
        });
    widget::container(widget::column(vec![title, contents.into()]).width(Length::Fill))
        .width(Length::Fill)
        .class(cosmic::theme::Container::custom(|theme| {
            widget::container::Style {
                border: crate::look::Look::normal().border(theme, crate::look::State::Focused),
                ..Default::default()
            }
        }))
        .into()
}

/// Leading columns reserved for every row in one menu or group. A `None`
/// omits the column for the whole group; an empty individual slot still uses it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowSlots {
    pub mark_width: Option<f32>,
    pub icon_width: Option<f32>,
    pub gap: f32,
}

impl Default for RowSlots {
    fn default() -> Self {
        Self {
            mark_width: None,
            icon_width: None,
            gap: 8.0,
        }
    }
}

/// Construct the inside of a row. The label and trailing content share one
/// flexible cell, so shortcuts and arrows are aligned at the row's right edge
/// without imposing separate global label and trailing columns.
pub fn row_content<'a, M: 'a>(
    slots: RowSlots,
    mark: Option<Element<'a, M>>,
    icon: Option<Element<'a, M>>,
    label: Element<'a, M>,
    trailing: Option<Element<'a, M>>,
) -> Element<'a, M> {
    let mut pieces: Vec<Element<'a, M>> = Vec::new();
    for (width, content) in [(slots.mark_width, mark), (slots.icon_width, icon)] {
        if let Some(width) = width {
            let content = content.unwrap_or_else(|| widget::Space::new().into());
            pieces.push(
                widget::container(content)
                    .width(Length::Fixed(width))
                    .into(),
            );
        }
    }
    // One flexible label allocation owns all remaining space. Adding another
    // Fill spacer here would split that offer when the label already fills it.
    let mut cell: Vec<Element<'a, M>> = vec![widget::container(label).width(Length::Fill).into()];
    if let Some(trailing) = trailing {
        cell.push(trailing);
    }
    pieces.push(
        widget::row(cell)
            .width(Length::Fill)
            .align_y(Alignment::Center)
            .spacing(slots.gap)
            .into(),
    );
    widget::row(pieces)
        .width(Length::Fill)
        .align_y(Alignment::Center)
        .spacing(slots.gap)
        .into()
}

/// An explicit tooltip decision for an actionable row, including a disabled
/// row's explanation. The tooltip is allowed to contain arbitrary widgets.
pub enum TooltipChoice<'a, M> {
    NoTooltip,
    Tooltip {
        content: Element<'a, M>,
        position: Position,
    },
}

/// Add an explicit tooltip decision to any row, including a custom-styled
/// button. The popup host forwards its nested overlay.
pub fn with_tooltip<'a, M: 'a>(
    content: Element<'a, M>,
    tooltip: TooltipChoice<'a, M>,
) -> Element<'a, M> {
    match tooltip {
        TooltipChoice::NoTooltip => content,
        TooltipChoice::Tooltip {
            content: tip,
            position,
        } => crate::tooltip_with(content, tip, position),
    }
}

/// A native button row. `None` disables activation while retaining the outer
/// hover target for the supplied explanation. Callers needing different button
/// styling can construct their own button and use [`with_tooltip`].
pub fn action_row<'a, M: Clone + 'a>(
    content: Element<'a, M>,
    tooltip: TooltipChoice<'a, M>,
    on_press: Option<M>,
) -> Element<'a, M> {
    action_row_with(content, tooltip, on_press, |button| button)
}

/// Configure the native button (for example, to assign a focus ID or style).
/// Tooltip positioning currently uses iced's overlay and is not yet planned
/// against the popup host's available screen space.
pub fn action_row_with<'a, M: Clone + 'a>(
    content: Element<'a, M>,
    tooltip: TooltipChoice<'a, M>,
    on_press: Option<M>,
    configure: impl FnOnce(
        widget::Button<'a, M, cosmic::Theme, cosmic::Renderer>,
    ) -> widget::Button<'a, M, cosmic::Theme, cosmic::Renderer>,
) -> Element<'a, M> {
    let button = widget::button(content)
        .width(Length::Fill)
        .on_press_maybe(on_press)
        .class(row_style(false, false, RowContour::Rounded));
    with_tooltip(configure(button).into(), tooltip)
}

/// Join arbitrary nonempty groups with caller-provided separators. The caller
/// controls card, list, separator, and row styling.
pub fn groups<'a, M: 'a>(
    groups: Vec<Vec<Element<'a, M>>>,
    mut separator: impl FnMut() -> Element<'a, M>,
) -> Element<'a, M> {
    let mut children = Vec::new();
    for group in groups.into_iter().filter(|group| !group.is_empty()) {
        if !children.is_empty() {
            children.push(separator());
        }
        children.extend(group);
    }
    widget::column(children).width(Length::Fill).into()
}

/// Compose an expandable section from an arbitrary header and children. The
/// header's action updates app-owned `expanded` state; this function publishes
/// no message and makes no decision about closing an enclosing popup.
pub fn expandable<'a, M: 'a>(
    header: Element<'a, M>,
    expanded: bool,
    children: Vec<Element<'a, M>>,
) -> Element<'a, M> {
    let mut contents = vec![header];
    if expanded {
        contents.extend(children);
    }
    widget::column(contents).width(Length::Fill).into()
}
