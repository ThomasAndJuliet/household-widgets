// SPDX-License-Identifier: GPL-3.0-only

//! Measured native representations, in increasing order of permitted compromise.
//!
//! The view factory must be free of application side effects: it is called for
//! disposable measurements as well as the selected representation. Queries use
//! fresh widget state and cannot consume the runtime's named widget state.
//! Compatible selected widgets retain their native state through ordinary tree
//! reconciliation. Changing widget types discards that local state, including
//! focus and inline overlays; this adapter does not transfer it between controls.

use cosmic::iced::advanced::widget::{Operation, Tree, tree};
use cosmic::iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use cosmic::iced::{Event, Length, Rectangle, Size, Vector};
use cosmic::{Element, Renderer, Theme, widget};

/// The maximum compromise a parent permits its content to make.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Level {
    #[default]
    Preferred,
    Mild,
    Strong,
    AllPermitted,
}

impl Level {
    /// Representations in preference order.
    pub const ALL: [Self; 4] = [
        Self::Preferred,
        Self::Mild,
        Self::Strong,
        Self::AllPermitted,
    ];
}

/// Query representations without retaining the result of any rejected query.
/// `None` explicitly means that none of the permitted representations fits.
pub fn first_fitting(
    max_level: Level,
    available: Size,
    measure: impl Fn(Level) -> Size,
) -> Option<Level> {
    Level::ALL
        .into_iter()
        .take_while(|level| *level <= max_level)
        .find(|level| {
            let size = measure(*level);
            size.width.is_finite()
                && size.height.is_finite()
                && size.width >= 0.0
                && size.height >= 0.0
                && size.width <= available.width
                && size.height <= available.height
        })
}

/// Width a child prefers, its lower bound, and its willingness to shrink.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WidthCapacity {
    pub preferred: f32,
    pub minimum: f32,
    /// Higher weights shrink faster; zero protects the preferred width.
    pub weight: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AllocationError {
    InvalidInput,
    /// The minima and zero-weight preferred widths exceed the available width.
    NoFit {
        required: f32,
        available: f32,
    },
}

/// Share a shortage using each child's original `weight * (preferred-minimum)`
/// rate, clamping minima and redistributing at the remaining original rates.
/// Surplus space is left unallocated. No representation levels are scheduled.
pub fn allocate_width(
    available: f32,
    capacities: &[WidthCapacity],
) -> Result<Vec<f32>, AllocationError> {
    if !available.is_finite()
        || available < 0.0
        || capacities.iter().any(|capacity| {
            !capacity.preferred.is_finite()
                || !capacity.minimum.is_finite()
                || !capacity.weight.is_finite()
                || capacity.minimum < 0.0
                || capacity.preferred < capacity.minimum
                || capacity.weight < 0.0
        })
    {
        return Err(AllocationError::InvalidInput);
    }
    let required: f64 = capacities
        .iter()
        .map(|capacity| {
            f64::from(if capacity.weight == 0.0 {
                capacity.preferred
            } else {
                capacity.minimum
            })
        })
        .sum();
    if required > f64::from(available) {
        return Err(AllocationError::NoFit {
            required: required as f32,
            available,
        });
    }
    let mut widths: Vec<f64> = capacities
        .iter()
        .map(|capacity| f64::from(capacity.preferred))
        .collect();
    let rates: Vec<f64> = capacities
        .iter()
        .map(|capacity| {
            f64::from(capacity.weight) * f64::from(capacity.preferred - capacity.minimum)
        })
        .collect();
    let mut shortage = (widths.iter().sum::<f64>() - f64::from(available)).max(0.0);
    while shortage > 0.0 {
        let total_rate: f64 = capacities
            .iter()
            .enumerate()
            .filter(|(i, capacity)| widths[*i] > f64::from(capacity.minimum))
            .map(|(i, _)| rates[i])
            .sum();
        if total_rate == 0.0 {
            break;
        }
        let mut removed = 0.0;
        let mut clamped = false;
        for (i, capacity) in capacities.iter().enumerate() {
            let room = widths[i] - f64::from(capacity.minimum);
            if room <= 0.0 || rates[i] == 0.0 {
                continue;
            }
            let share = shortage * rates[i] / total_rate;
            let reduction = share.min(room);
            widths[i] -= reduction;
            removed += reduction;
            clamped |= share >= room;
        }
        if !clamped || removed == 0.0 {
            break;
        }
        shortage = (shortage - removed).max(0.0);
    }
    Ok(widths.into_iter().map(|width| width as f32).collect())
}

/// Choose the first permitted representation whose measured width fits.
///
/// If none fits, the last permitted representation receives the available
/// constraints. Its native controls determine clipping or wrapping; fallback is
/// not a claim that the representation fits. Measurements are intrinsic sizes.
/// This first adapter supports one fixed layout with increasing levels; a parent
/// that tries different arrangements can use [`first_fitting`] for each stage.
pub fn responsive<'a, Message: 'a>(
    max_level: Level,
    view: impl Fn(Level) -> Element<'a, Message> + 'a,
) -> ResponsiveLevels<'a, Message> {
    ResponsiveLevels {
        view: Box::new(view),
        max_level,
        width: Length::Fill,
        height: Length::Shrink,
        content: widget::space().into(),
    }
}

/// A native widget whose representations are measured with the real renderer.
pub struct ResponsiveLevels<'a, Message> {
    view: Box<dyn Fn(Level) -> Element<'a, Message> + 'a>,
    max_level: Level,
    width: Length,
    height: Length,
    content: Element<'a, Message>,
}

impl<Message> ResponsiveLevels<'_, Message> {
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }
}

// Tree::diff and layout-time diffing (e.g. a nested native Responsive) consult
// this runtime map. A fresh Tree alone would let a discarded probe steal focus
// or popup state from the committed UI. Restore even when a factory panics.
struct NamedStateGuard<F: FnOnce()>(Option<F>);

fn isolate_named_state() -> impl Drop {
    let saved = tree::NAMED.with(|named| std::mem::take(&mut *named.borrow_mut()));
    NamedStateGuard(Some(move || {
        tree::NAMED.with(|named| *named.borrow_mut() = saved)
    }))
}

impl<F: FnOnce()> Drop for NamedStateGuard<F> {
    fn drop(&mut self) {
        if let Some(restore) = self.0.take() {
            restore();
        }
    }
}

impl<Message> Widget<Message, Theme, Renderer> for ResponsiveLevels<'_, Message> {
    fn diff(&mut self, _tree: &mut Tree) {
        // The parent has not allocated width yet. Reconcile the selected
        // representation in layout, like libcosmic's native Responsive.
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.width(self.width).height(self.height);
        let available = limits.max();
        let measurement_limits =
            layout::Limits::with_compression(Size::ZERO, Size::INFINITE, Size::new(true, true));
        let selected = first_fitting(self.max_level, available, |level| {
            let _isolation = isolate_named_state();
            let mut candidate = (self.view)(level);
            let mut candidate_tree = Tree::new(candidate.as_widget());
            // Native dropdown initializes its measured paragraphs in diff.
            candidate_tree.diff(candidate.as_widget_mut());
            candidate
                .as_widget_mut()
                .layout(&mut candidate_tree, renderer, &measurement_limits)
                .size()
        })
        .unwrap_or(self.max_level);
        self.content = (self.view)(selected);
        tree.diff_children(std::slice::from_mut(&mut self.content));
        let node =
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, &limits.loose());
        layout::Node::with_children(
            limits.resolve(self.width, self.height, node.size()),
            vec![node],
        )
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            child_layout(layout),
            cursor,
            viewport,
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
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            child_layout(layout),
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
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
            child_layout(layout),
            renderer,
            operation,
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
            child_layout(layout),
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            child_layout(layout),
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
        destinations: &mut cosmic::iced::advanced::clipboard::DndDestinationRectangles,
    ) {
        self.content.as_widget().drag_destinations(
            &tree.children[0],
            child_layout(layout),
            renderer,
            destinations,
        );
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
            .a11y_nodes(child_layout(layout), &tree.children[0], cursor)
    }
}

fn child_layout(layout: Layout<'_>) -> Layout<'_> {
    layout
        .children()
        .next()
        .expect("responsive representation was laid out")
        .with_virtual_offset(layout.virtual_offset())
}

impl<'a, Message: 'a> From<ResponsiveLevels<'a, Message>> for Element<'a, Message> {
    fn from(responsive: ResponsiveLevels<'a, Message>) -> Self {
        Self::new(responsive)
    }
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
