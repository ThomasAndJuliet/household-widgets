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
use std::rc::Rc;

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
        query: None,
    }
}

/// A native widget whose representations are measured with the real renderer.
pub struct ResponsiveLevels<'a, Message> {
    view: Box<dyn Fn(Level) -> Element<'a, Message> + 'a>,
    max_level: Level,
    width: Length,
    height: Length,
    content: Element<'a, Message>,
    query: Option<SizeQuery<'a>>,
}

/// Intrinsic content size and the smallest permitted allocated box at one level.
/// A scrolling viewport can have a smaller minimum than its full content width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasuredSize {
    pub preferred: Size,
    pub minimum: Size,
}

type SizeQuery<'a> = Rc<dyn Fn(Level, &Renderer) -> MeasuredSize + 'a>;

/// A queryable leaf or composite. Only the root selects a level; composites
/// query and render every descendant at exactly the level their parent supplies.
pub struct Child<'a, Message> {
    view: Rc<dyn Fn(Level) -> Element<'a, Message> + 'a>,
    query: SizeQuery<'a>,
    weight: f32,
    grow: f32,
}

impl<Message> Clone for Child<'_, Message> {
    fn clone(&self) -> Self {
        Self {
            view: self.view.clone(),
            query: self.query.clone(),
            weight: self.weight,
            grow: self.grow,
        }
    }
}

impl<'a, Message: 'a> Child<'a, Message> {
    pub fn view(&self, level: Level) -> Element<'a, Message> {
        (self.view)(level)
    }
    pub fn query(&self, level: Level, renderer: &Renderer) -> MeasuredSize {
        (self.query)(level, renderer)
    }

    pub fn weight(mut self, weight: f32) -> Self {
        assert!(weight.is_finite() && weight >= 0.0);
        self.weight = weight;
        self
    }

    /// Share surplus space after preferred widths fit. Zero keeps intrinsic width.
    pub fn grow(mut self, grow: f32) -> Self {
        assert!(grow.is_finite() && grow >= 0.0);
        self.grow = grow;
        self
    }

    /// Reserve whole logical-pixel widths for controls whose origins must align.
    /// Native measurement stays unchanged; only this child's size offers round up.
    pub fn round_width_up(mut self) -> Self {
        let query = self.query.clone();
        self.query = Rc::new(move |level, renderer| {
            let mut measured = query(level, renderer);
            measured.preferred.width = measured.preferred.width.ceil();
            measured.minimum.width = measured.minimum.width.ceil();
            measured
        });
        self
    }

    /// The view must remain usable at this width without exceeding its queried
    /// height. A scrolling viewport satisfies this contract; arbitrary wrapping
    /// text needs its own height-aware query instead of this width-only override.
    pub fn minimum_width(mut self, minimum: impl Fn(Level, Size) -> f32 + 'a) -> Self {
        let query = self.query.clone();
        self.query = Rc::new(move |level, renderer| {
            let mut measured = query(level, renderer);
            measured.minimum.width =
                minimum(level, measured.preferred).clamp(0.0, measured.preferred.width);
            measured
        });
        self
    }
}

/// A native leaf measured with fresh, isolated state on every query.
pub fn child<'a, Message: 'a>(
    view: impl Fn(Level) -> Element<'a, Message> + 'a,
) -> Child<'a, Message> {
    let view: Rc<dyn Fn(Level) -> Element<'a, Message> + 'a> = Rc::new(view);
    let measure = view.clone();
    Child {
        view,
        query: Rc::new(move |level, renderer| {
            let size = measure_element(|| measure(level), renderer);
            MeasuredSize {
                preferred: size,
                minimum: size,
            }
        }),
        weight: 1.0,
        grow: 0.0,
    }
}

/// An elastic gap with a fixed minimum and a share of surplus space.
/// It reports its minimum during queries; the parent allocates its rendered width.
pub fn spacer<'a, Message: 'a>(minimum: f32) -> Child<'a, Message> {
    assert!(minimum.is_finite() && minimum >= 0.0);
    Child {
        view: Rc::new(|_| widget::Space::new().width(Length::Fill).height(0).into()),
        query: Rc::new(move |_, _| MeasuredSize {
            preferred: Size::new(minimum, 0.0),
            minimum: Size::new(minimum, 0.0),
        }),
        weight: 0.0,
        grow: 1.0,
    }
}

/// Compose a row that remains queryable at a parent-supplied fixed level.
pub fn row_child<'a, Message: 'a>(
    children: Vec<Child<'a, Message>>,
    spacing: f32,
) -> Child<'a, Message> {
    assert!(spacing.is_finite() && spacing >= 0.0);
    let measured_children = children.clone();
    Child {
        view: Rc::new(move |level| {
            Element::new(FixedRow {
                children: children.clone(),
                level,
                spacing,
                content: widget::space().into(),
            })
        }),
        query: Rc::new(move |level, renderer| {
            let gap = spacing * measured_children.len().saturating_sub(1) as f32;
            let mut result = MeasuredSize {
                preferred: Size::new(gap, 0.0),
                minimum: Size::new(gap, 0.0),
            };
            for child in &measured_children {
                let size = child.query(level, renderer);
                result.preferred.width += size.preferred.width;
                result.preferred.height = result.preferred.height.max(size.preferred.height);
                result.minimum.width += if child.weight == 0.0 {
                    size.preferred.width
                } else {
                    size.minimum.width
                };
                result.minimum.height = result.minimum.height.max(size.minimum.height);
            }
            result
        }),
        weight: 1.0,
        grow: 0.0,
    }
}

/// Select one level for an entire recursive composition, then allocate widths.
pub fn responsive_child<'a, Message: 'a>(
    max_level: Level,
    child: Child<'a, Message>,
) -> ResponsiveLevels<'a, Message> {
    let query = child.query.clone();
    let mut responsive = responsive(max_level, move |level| (child.view)(level));
    responsive.query = Some(query);
    responsive
}

pub fn row<'a, Message: 'a>(
    max_level: Level,
    children: Vec<Child<'a, Message>>,
    spacing: f32,
) -> ResponsiveLevels<'a, Message> {
    responsive_child(max_level, row_child(children, spacing))
}

struct FixedRow<'a, Message> {
    children: Vec<Child<'a, Message>>,
    level: Level,
    spacing: f32,
    content: Element<'a, Message>,
}

impl<'a, Message: 'a> Widget<Message, Theme, Renderer> for FixedRow<'a, Message> {
    forward_widget!(Message, Theme, Renderer; draw, update, operate, mouse_interaction, overlay, drag_destinations);

    fn diff(&mut self, _tree: &mut Tree) {}

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let measurements: Vec<_> = self
            .children
            .iter()
            .map(|child| child.query(self.level, renderer))
            .collect();
        let capacities: Vec<_> = self
            .children
            .iter()
            .zip(&measurements)
            .map(|(child, size)| WidthCapacity {
                preferred: size.preferred.width,
                minimum: size.minimum.width,
                weight: child.weight,
            })
            .collect();
        let available = (limits.max().width
            - self.spacing * self.children.len().saturating_sub(1) as f32)
            .max(0.0);
        let mut widths: Vec<f32> = if available.is_infinite() {
            capacities
                .iter()
                .map(|capacity| capacity.preferred)
                .collect()
        } else {
            allocate_width(available, &capacities).unwrap_or_else(|_| {
                capacities
                    .iter()
                    .map(|capacity| {
                        if capacity.weight == 0.0 {
                            capacity.preferred
                        } else {
                            capacity.minimum
                        }
                    })
                    .collect()
            })
        };
        let total_grow: f64 = self
            .children
            .iter()
            .map(|child| f64::from(child.grow))
            .sum();
        if available.is_finite() && total_grow > 0.0 {
            let used: f64 = widths.iter().map(|width| f64::from(*width)).sum();
            let surplus = (f64::from(available) - used).max(0.0);
            for (width, child) in widths.iter_mut().zip(&self.children) {
                *width += (surplus * f64::from(child.grow) / total_grow) as f32;
            }
        }
        self.content = widget::row::with_children(
            self.children
                .iter()
                .zip(widths)
                .map(|(child, width)| {
                    widget::container((child.view)(self.level))
                        .width(Length::Fixed(width))
                        .into()
                })
                .collect::<Vec<Element<'_, Message>>>(),
        )
        .spacing(self.spacing)
        .align_y(cosmic::iced::Alignment::Center)
        .into();
        tree.diff_children(std::slice::from_mut(&mut self.content));
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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

pub(crate) fn measure_element<'a, Message>(
    view: impl FnOnce() -> Element<'a, Message>,
    renderer: &Renderer,
) -> Size {
    let _isolation = isolate_named_state();
    let mut candidate = view();
    let mut tree = Tree::new(candidate.as_widget());
    tree.diff(candidate.as_widget_mut());
    candidate
        .as_widget_mut()
        .layout(
            &mut tree,
            renderer,
            &layout::Limits::with_compression(Size::ZERO, Size::INFINITE, Size::new(true, true)),
        )
        .size()
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
        let selected = first_fitting(self.max_level, available, |level| {
            self.query.as_ref().map_or_else(
                || measure_element(|| (self.view)(level), renderer),
                |query| query(level, renderer).minimum,
            )
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
