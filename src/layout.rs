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
use std::cell::Cell;
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

/// Preferred and useful minimum geometry of a component at one exact level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasuredSize {
    pub preferred: Size,
    pub minimum: Size,
}

type SizeQuery<'a> = Rc<dyn Fn(Level, &Renderer) -> MeasuredSize + 'a>;

/// An arrangement applies only to immediate children, which remain atomic.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arrangement {
    Row,
    Column,
    Wrap,
}

/// Geometry proposed by a pure query. No widget state is retained here.
#[derive(Clone, Debug, PartialEq)]
pub struct GeometryPlan {
    pub size: Size,
    pub hidden: bool,
    pub children: Vec<Placement>,
    // Row membership is part of the plan, not recomputed during commit.
    rows: Vec<Vec<usize>>,
    minimum_height: f32,
    native_size: Option<Size>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    /// Index in the declared immediate children (hidden children are omitted).
    pub index: usize,
    pub position: cosmic::iced::Point,
    pub geometry: GeometryPlan,
}

impl GeometryPlan {
    fn leaf(size: Size) -> Self {
        Self {
            size,
            hidden: false,
            children: Vec::new(),
            rows: Vec::new(),
            minimum_height: size.height,
            native_size: None,
        }
    }
    fn hidden() -> Self {
        Self {
            hidden: true,
            ..Self::leaf(Size::ZERO)
        }
    }
}

struct Composition<'a, Message> {
    children: Vec<Child<'a, Message>>,
    arrangement: Arrangement,
    spacing: f32,
}

/// A queryable leaf or composite. Only the root selects a level. Composites
/// forward that exact level; outward weights never replace internal weights.
pub struct Child<'a, Message> {
    view: Rc<dyn Fn(Level) -> Element<'a, Message> + 'a>,
    query: SizeQuery<'a>,
    composition: Option<Rc<Composition<'a, Message>>>,
    forms: Option<Rc<Vec<(Level, Child<'a, Message>)>>>,
    hidden: Rc<dyn Fn(Level) -> bool + 'a>,
    viewport: Option<Size>,
    weight: f32,
    grow: f32,
    grow_height: f32,
}

impl<Message> Clone for Child<'_, Message> {
    fn clone(&self) -> Self {
        Self {
            view: self.view.clone(),
            query: self.query.clone(),
            composition: self.composition.clone(),
            forms: self.forms.clone(),
            hidden: self.hidden.clone(),
            viewport: self.viewport,
            weight: self.weight,
            grow: self.grow,
            grow_height: self.grow_height,
        }
    }
}

impl<'a, Message: 'a> Child<'a, Message> {
    fn selected_form(&self, level: Level) -> Option<&Self> {
        self.forms.as_ref().map(|forms| select_form(forms, level))
    }

    fn has_composition(&self, level: Level) -> bool {
        self.composition.is_some()
            || self
                .selected_form(level)
                .is_some_and(|form| form.has_composition(level))
    }

    fn is_hidden(&self, level: Level) -> bool {
        (self.hidden)(level)
            || self
                .selected_form(level)
                .is_some_and(|form| form.is_hidden(level))
            || self.composition.as_ref().is_some_and(|composition| {
                composition
                    .children
                    .iter()
                    .all(|child| child.is_hidden(level))
            })
    }

    pub fn view(&self, level: Level) -> Element<'a, Message> {
        Element::new(FixedComponent {
            child: self.clone(),
            level,
            content: widget::space().into(),
        })
    }

    pub fn query(&self, level: Level, renderer: &Renderer) -> MeasuredSize {
        if self.is_hidden(level) {
            return MeasuredSize {
                preferred: Size::ZERO,
                minimum: Size::ZERO,
            };
        }
        (self.query)(level, renderer)
    }

    /// Allocate first, then measure native content at its allocated width with
    /// unbounded height. Thus a clipped height can never certify a fit. A useful
    /// minimum narrower than intrinsic width must be explicitly declared.
    pub fn query_at(
        &self,
        level: Level,
        renderer: &Renderer,
        available: Size,
    ) -> Result<GeometryPlan, AllocationError> {
        if !valid_available(available) {
            return Err(AllocationError::InvalidInput);
        }
        if self.is_hidden(level) {
            return Ok(GeometryPlan::hidden());
        }
        let measured = self.query(level, renderer);
        validate_measurement(measured)?;
        let widths = allocate_with_growth(
            available.width,
            &[WidthCapacity {
                preferred: measured.preferred.width,
                minimum: measured.minimum.width,
                weight: self.weight,
            }],
            &[self.grow],
        )?;
        let width = if self.has_composition(level) && available.width.is_finite() {
            available.width
        } else {
            widths[0]
        };
        self.plan_width(level, renderer, width, available.height)
    }

    fn plan_width(
        &self,
        level: Level,
        renderer: &Renderer,
        width: f32,
        height: f32,
    ) -> Result<GeometryPlan, AllocationError> {
        if self.is_hidden(level) {
            return Ok(GeometryPlan::hidden());
        }
        let mut plan = if let Some(form) = self.selected_form(level) {
            let mut selected = form.clone();
            if self.grow_height > 0.0 {
                selected.grow_height = self.grow_height;
            }
            selected.plan_width(level, renderer, width, height)?
        } else if let Some(composition) = &self.composition {
            composition.plan(level, renderer, width, height)?
        } else {
            let viewport_height = self.viewport.map(|minimum| {
                let measured = self.query(level, renderer);
                if self.grow_height > 0.0 && height.is_finite() {
                    height.max(minimum.height)
                } else {
                    measured.preferred.height.min(height).max(minimum.height)
                }
            });
            let size = measure_element_in(
                || (self.view)(level),
                renderer,
                Size::new(width, viewport_height.unwrap_or(f32::INFINITY)),
                viewport_height.is_none(),
            );
            if !finite_size(size) {
                return Err(AllocationError::InvalidInput);
            }
            if size.width > width + FIT_EPSILON {
                return Err(AllocationError::NoFit {
                    required: size.width,
                    available: width,
                });
            }
            if let Some(offered_height) = viewport_height {
                if size.height > offered_height + FIT_EPSILON {
                    return Err(AllocationError::NoFit {
                        required: size.height,
                        available: offered_height,
                    });
                }
            }
            // The parent reserves the allocated width even for shrink content.
            let mut plan = GeometryPlan::leaf(Size::new(width, size.height));
            plan.native_size = Some(size);
            if let Some(minimum) = self.viewport {
                plan.size.height = viewport_height.expect("declared viewport height");
                plan.minimum_height = minimum.height;
            }
            plan
        };
        if self.grow_height > 0.0 && height.is_finite() {
            plan.size.height = plan.size.height.max(height);
        }
        if plan.size.width > width + FIT_EPSILON {
            return Err(AllocationError::NoFit {
                required: plan.size.width,
                available: width,
            });
        }
        if plan.size.height > height + FIT_EPSILON {
            return Err(AllocationError::NoFit {
                required: plan.size.height,
                available: height,
            });
        }
        Ok(plan)
    }

    fn render(
        &self,
        level: Level,
        plan: &GeometryPlan,
        agreement: Rc<Cell<bool>>,
    ) -> Element<'a, Message> {
        if plan.hidden {
            return widget::space().width(0).height(0).into();
        }
        if let Some(form) = self.selected_form(level) {
            let mut selected = form.clone();
            if self.grow_height > 0.0 {
                selected.grow_height = self.grow_height;
            }
            let content = selected.render(level, plan, agreement);
            return if self.grow_height > 0.0 {
                widget::container(content)
                    .width(plan.size.width)
                    .height(plan.size.height)
                    .into()
            } else {
                content
            };
        }
        let Some(composition) = &self.composition else {
            let content = Element::new(VerifiedLeaf {
                content: (self.view)(level),
                expected: plan.native_size.expect("native leaf measurement"),
                maximum: Size::new(
                    plan.size.width,
                    if self.viewport.is_some() {
                        plan.size.height
                    } else {
                        f32::INFINITY
                    },
                ),
                compress_height: self.viewport.is_none(),
                agreement,
            });
            return widget::container(content)
                .width(plan.size.width)
                .height(plan.size.height)
                .into();
        };
        let make_row = |indices: &[usize]| -> Element<'a, Message> {
            widget::row::with_children(
                indices
                    .iter()
                    .map(|i| {
                        let placed = &plan.children[*i];
                        widget::container(composition.children[placed.index].render(
                            level,
                            &placed.geometry,
                            agreement.clone(),
                        ))
                        .width(placed.geometry.size.width)
                        .height(placed.geometry.size.height)
                        .into()
                    })
                    .collect::<Vec<Element<'a, Message>>>(),
            )
            .spacing(composition.spacing)
            .align_y(cosmic::iced::Alignment::Center)
            .into()
        };
        let content = match composition.arrangement {
            Arrangement::Row => make_row(plan.rows.first().map_or(&[], Vec::as_slice)),
            Arrangement::Wrap => widget::column::with_children(
                plan.rows
                    .iter()
                    .map(|row| make_row(row))
                    .collect::<Vec<_>>(),
            )
            .spacing(composition.spacing)
            .into(),
            Arrangement::Column => widget::column::with_children(
                plan.children
                    .iter()
                    .map(|placed| {
                        widget::container(composition.children[placed.index].render(
                            level,
                            &placed.geometry,
                            agreement.clone(),
                        ))
                        .width(placed.geometry.size.width)
                        .height(placed.geometry.size.height)
                        .into()
                    })
                    .collect::<Vec<Element<'a, Message>>>(),
            )
            .spacing(composition.spacing)
            .into(),
        };
        if self.grow_height > 0.0 {
            widget::container(content)
                .width(plan.size.width)
                .height(plan.size.height)
                .into()
        } else {
            content
        }
    }

    pub fn weight(mut self, weight: f32) -> Self {
        assert!(weight.is_finite() && weight >= 0.0);
        self.weight = weight;
        self
    }

    /// Share surplus only after shrinking has finished.
    pub fn grow(mut self, grow: f32) -> Self {
        assert!(grow.is_finite() && grow >= 0.0);
        self.grow = grow;
        self
    }

    /// Share surplus height in a column independently of width growth. Native
    /// content keeps its measured height floor; a declared viewport expands.
    pub fn grow_height(mut self, grow: f32) -> Self {
        assert!(grow.is_finite() && grow >= 0.0);
        self.grow_height = grow;
        self
    }

    /// Explicit component permission to omit this form. No layout slot, gap,
    /// event target, overlay or accessibility node is built for hidden children.
    pub fn hidden_when(mut self, hidden: impl Fn(Level) -> bool + 'a) -> Self {
        self.hidden = Rc::new(hidden);
        self
    }

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

    /// Declare a usable allocated width. Native content is measured again at
    /// that width, so wrapping text contributes its actual resulting height.
    /// The component is responsible for making the narrower form usable.
    pub fn minimum_width(mut self, minimum: impl Fn(Level, Size) -> f32 + 'a) -> Self {
        let query = self.query.clone();
        self.query = Rc::new(move |level, renderer| {
            let mut measured = query(level, renderer);
            measured.minimum.width =
                if measured.preferred.width.is_finite() && measured.preferred.width >= 0.0 {
                    minimum(level, measured.preferred).clamp(0.0, measured.preferred.width)
                } else {
                    f32::NAN
                };
            measured
        });
        self
    }
}

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
        composition: None,
        forms: None,
        hidden: Rc::new(|_| false),
        viewport: None,
        weight: 1.0,
        grow: 0.0,
        grow_height: 0.0,
    }
}

/// A component family's own structured forms, inherited until the next
/// threshold. The first threshold must be Preferred and thresholds strictly
/// increase. Every selected form receives the requested level, so an inherited
/// composite still forwards Mild/Strong rules to its own immediate children.
/// The returned child's outward weights are independent of the form's internals.
pub fn forms<'a, Message: 'a>(
    alternatives: Vec<(Level, Child<'a, Message>)>,
) -> Child<'a, Message> {
    assert!(
        alternatives
            .first()
            .is_some_and(|(level, _)| *level == Level::Preferred)
    );
    assert!(alternatives.windows(2).all(|pair| pair[0].0 < pair[1].0));
    let alternatives = Rc::new(alternatives);
    let query_forms = alternatives.clone();
    Child {
        view: Rc::new(|_| widget::space().into()),
        query: Rc::new(move |level, renderer| {
            select_form(&query_forms, level).query(level, renderer)
        }),
        composition: None,
        forms: Some(alternatives),
        hidden: Rc::new(|_| false),
        viewport: None,
        weight: 1.0,
        grow: 0.0,
        grow_height: 0.0,
    }
}

fn select_form<'a, 'forms, Message>(
    alternatives: &'forms [(Level, Child<'a, Message>)],
    level: Level,
) -> &'forms Child<'a, Message> {
    &alternatives
        .iter()
        .rev()
        .find(|(threshold, _)| *threshold <= level)
        .expect("Preferred form is always inherited")
        .1
}

/// Declare a native scrolling viewport, including its scrollbar and any other
/// controls in `view`. The declaration is local; no stage invents scrolling.
/// `view` must actually implement scrolling at the offered viewport dimensions.
pub fn scroll_child<'a, Message: 'a>(
    view: impl Fn(Level) -> Element<'a, Message> + 'a,
    minimum: Size,
) -> Child<'a, Message> {
    assert!(finite_size(minimum));
    let mut child = child(view);
    let query = child.query.clone();
    child.query = Rc::new(move |level, renderer| {
        let measured = query(level, renderer);
        MeasuredSize {
            preferred: Size::new(
                measured.preferred.width.max(minimum.width),
                measured.preferred.height.max(minimum.height),
            ),
            minimum,
        }
    });
    child.viewport = Some(minimum);
    child
}

pub fn spacer<'a, Message: 'a>(minimum: f32) -> Child<'a, Message> {
    assert!(minimum.is_finite() && minimum >= 0.0);
    child(|_| widget::Space::new().width(Length::Fill).height(0).into())
        .with_measurement(move |_, _| MeasuredSize {
            preferred: Size::new(minimum, 0.0),
            minimum: Size::new(minimum, 0.0),
        })
        .weight(0.0)
        .grow(1.0)
}

impl<'a, Message: 'a> Child<'a, Message> {
    fn with_measurement(mut self, query: impl Fn(Level, &Renderer) -> MeasuredSize + 'a) -> Self {
        self.query = Rc::new(query);
        self
    }
}

pub fn row_child<'a, Message: 'a>(
    children: Vec<Child<'a, Message>>,
    spacing: f32,
) -> Child<'a, Message> {
    compose(children, spacing, Arrangement::Row)
}

pub fn column_child<'a, Message: 'a>(
    children: Vec<Child<'a, Message>>,
    spacing: f32,
) -> Child<'a, Message> {
    compose(children, spacing, Arrangement::Column)
}

pub fn wrap_child<'a, Message: 'a>(
    children: Vec<Child<'a, Message>>,
    spacing: f32,
) -> Child<'a, Message> {
    compose(children, spacing, Arrangement::Wrap)
}

fn compose<'a, Message: 'a>(
    children: Vec<Child<'a, Message>>,
    spacing: f32,
    arrangement: Arrangement,
) -> Child<'a, Message> {
    assert!(spacing.is_finite() && spacing >= 0.0);
    let composition = Rc::new(Composition {
        children,
        arrangement,
        spacing,
    });
    let query = composition.clone();
    Child {
        view: Rc::new(|_| widget::space().into()),
        query: Rc::new(move |level, renderer| query.measure(level, renderer)),
        composition: Some(composition),
        forms: None,
        hidden: Rc::new(|_| false),
        viewport: None,
        weight: 1.0,
        grow: 0.0,
        grow_height: 0.0,
    }
}

const FIT_EPSILON: f32 = 0.001;
fn finite_size(size: Size) -> bool {
    size.width.is_finite() && size.height.is_finite() && size.width >= 0.0 && size.height >= 0.0
}
fn valid_available(size: Size) -> bool {
    !size.width.is_nan() && !size.height.is_nan() && size.width >= 0.0 && size.height >= 0.0
}
fn validate_measurement(size: MeasuredSize) -> Result<(), AllocationError> {
    if finite_size(size.preferred)
        && finite_size(size.minimum)
        && size.minimum.width <= size.preferred.width
        && size.minimum.height <= size.preferred.height
    {
        Ok(())
    } else {
        Err(AllocationError::InvalidInput)
    }
}
fn allocate_with_growth(
    available: f32,
    capacities: &[WidthCapacity],
    grows: &[f32],
) -> Result<Vec<f32>, AllocationError> {
    let mut widths = if available.is_infinite() {
        if capacities
            .iter()
            .any(|capacity| !finite_size(Size::new(capacity.preferred, capacity.minimum)))
        {
            return Err(AllocationError::InvalidInput);
        }
        capacities
            .iter()
            .map(|capacity| capacity.preferred)
            .collect()
    } else {
        allocate_width(available, capacities)?
    };
    let total: f64 = grows.iter().map(|grow| f64::from(*grow)).sum();
    if available.is_finite() && total > 0.0 {
        let surplus = (f64::from(available)
            - widths.iter().map(|width| f64::from(*width)).sum::<f64>())
        .max(0.0);
        for (width, grow) in widths.iter_mut().zip(grows) {
            *width += (surplus * f64::from(*grow) / total) as f32;
        }
    }
    Ok(widths)
}

impl<'a, Message: 'a> Composition<'a, Message> {
    fn measure(&self, level: Level, renderer: &Renderer) -> MeasuredSize {
        let measured: Vec<_> = self
            .children
            .iter()
            .filter(|child| !child.is_hidden(level))
            .map(|child| (child, child.query(level, renderer)))
            .collect();
        let gap = self.spacing * measured.len().saturating_sub(1) as f32;
        let mut result = MeasuredSize {
            preferred: Size::ZERO,
            minimum: Size::ZERO,
        };
        for (child, size) in &measured {
            let minimum_width = if child.weight == 0.0 {
                size.preferred.width
            } else {
                size.minimum.width
            };
            match self.arrangement {
                Arrangement::Row | Arrangement::Wrap => {
                    result.preferred.width += size.preferred.width;
                    result.preferred.height = result.preferred.height.max(size.preferred.height);
                    if self.arrangement == Arrangement::Row {
                        result.minimum.width += minimum_width;
                        result.minimum.height = result.minimum.height.max(size.minimum.height);
                    } else {
                        result.minimum.width = result.minimum.width.max(minimum_width);
                        // A height bound, not the height at that width. plan() measures rows.
                        result.minimum.height = result.minimum.height.max(size.minimum.height);
                    }
                }
                Arrangement::Column => {
                    result.preferred.width = result.preferred.width.max(size.preferred.width);
                    result.minimum.width = result.minimum.width.max(minimum_width);
                    result.preferred.height += size.preferred.height;
                    result.minimum.height += size.minimum.height;
                }
            }
        }
        match self.arrangement {
            Arrangement::Row => {
                result.preferred.width += gap;
                result.minimum.width += gap;
            }
            Arrangement::Wrap => result.preferred.width += gap,
            Arrangement::Column => {
                result.preferred.height += gap;
                result.minimum.height += gap;
            }
        }
        result
    }

    fn plan(
        &self,
        level: Level,
        renderer: &Renderer,
        width: f32,
        height: f32,
    ) -> Result<GeometryPlan, AllocationError> {
        let active: Vec<_> = self
            .children
            .iter()
            .enumerate()
            .filter(|(_, child)| !child.is_hidden(level))
            .map(|(index, child)| (index, child.query(level, renderer)))
            .collect();
        for (_, size) in &active {
            validate_measurement(*size)?;
        }
        let capacity = |index: usize, measured: MeasuredSize| WidthCapacity {
            preferred: measured.preferred.width,
            minimum: measured.minimum.width,
            weight: self.children[index].weight,
        };
        let mut result = GeometryPlan::leaf(Size::ZERO);
        if self.arrangement == Arrangement::Column {
            let gap = self.spacing * active.len().saturating_sub(1) as f32;
            if gap > height {
                return Err(AllocationError::NoFit {
                    required: gap,
                    available: height,
                });
            }
            let mut proposals = Vec::new();
            for (index, measured) in active {
                let child = &self.children[index];
                let allocated =
                    allocate_with_growth(width, &[capacity(index, measured)], &[child.grow])?[0];
                let geometry = child.plan_width(level, renderer, allocated, f32::INFINITY)?;
                proposals.push((index, allocated, geometry));
            }
            // Width-dependent ordinary heights are hard floors. Only explicitly
            // declared viewports (possibly nested) offer vertical capacity.
            let heights = allocate_with_growth(
                height - gap,
                &proposals
                    .iter()
                    .map(|(index, _, plan)| WidthCapacity {
                        preferred: plan.size.height,
                        minimum: plan.minimum_height,
                        weight: self.children[*index].weight,
                    })
                    .collect::<Vec<_>>(),
                &proposals
                    .iter()
                    .map(|(index, _, _)| self.children[*index].grow_height)
                    .collect::<Vec<_>>(),
            )?;
            result.minimum_height = gap;
            for ((index, allocated, proposal), allocated_height) in
                proposals.into_iter().zip(heights)
            {
                let geometry = if (allocated_height - proposal.size.height).abs() > FIT_EPSILON {
                    self.children[index].plan_width(level, renderer, allocated, allocated_height)?
                } else {
                    proposal
                };
                if !result.children.is_empty() {
                    result.size.height += self.spacing;
                }
                let position = cosmic::iced::Point::new(0.0, result.size.height);
                result.size.height += geometry.size.height;
                result.size.width = result.size.width.max(geometry.size.width);
                result.minimum_height += geometry.minimum_height;
                result.children.push(Placement {
                    index,
                    position,
                    geometry,
                });
            }
            return Ok(result);
        }
        // Pack using useful minima, including zero-weight protection. Each new
        // row gets a fresh allocation from that stage's preferred dimensions.
        let mut rows: Vec<Vec<(usize, MeasuredSize)>> = Vec::new();
        let mut used = 0.0;
        for (index, measured) in active {
            let child = &self.children[index];
            let minimum = if child.weight == 0.0 {
                measured.preferred.width
            } else {
                measured.minimum.width
            };
            if minimum > width + FIT_EPSILON {
                return Err(AllocationError::NoFit {
                    required: minimum,
                    available: width,
                });
            }
            let extra = if rows.last().is_some_and(|row| !row.is_empty()) {
                self.spacing
            } else {
                0.0
            };
            if self.arrangement == Arrangement::Wrap && used + extra + minimum > width + FIT_EPSILON
            {
                rows.push(Vec::new());
                used = 0.0;
            }
            if rows.is_empty() {
                rows.push(Vec::new());
            }
            if !rows.last().unwrap().is_empty() {
                used += self.spacing;
            }
            used += minimum;
            rows.last_mut().unwrap().push((index, measured));
        }
        for row in rows {
            let gap = self.spacing * row.len().saturating_sub(1) as f32;
            if gap > width {
                return Err(AllocationError::NoFit {
                    required: gap,
                    available: width,
                });
            }
            let capacities: Vec<_> = row
                .iter()
                .map(|(index, measured)| capacity(*index, *measured))
                .collect();
            let grows: Vec<_> = row
                .iter()
                .map(|(index, _)| self.children[*index].grow)
                .collect();
            let widths = allocate_with_growth(width - gap, &capacities, &grows)?;
            let start = result.children.len();
            let mut row_width = 0.0;
            let mut row_height: f32 = 0.0;
            let mut row_minimum_height: f32 = 0.0;
            for ((index, _), allocated) in row.into_iter().zip(widths) {
                let geometry = self.children[index].plan_width(
                    level,
                    renderer,
                    allocated,
                    if self.arrangement == Arrangement::Row {
                        height
                    } else {
                        f32::INFINITY
                    },
                )?;
                if result.children.len() > start {
                    row_width += self.spacing;
                }
                let position = cosmic::iced::Point::new(row_width, result.size.height);
                row_width += geometry.size.width;
                row_height = row_height.max(geometry.size.height);
                row_minimum_height = row_minimum_height.max(geometry.minimum_height);
                result.children.push(Placement {
                    index,
                    position,
                    geometry,
                });
            }
            for placed in &mut result.children[start..] {
                placed.position.y += (row_height - placed.geometry.size.height) / 2.0;
            }
            result.rows.push((start..result.children.len()).collect());
            result.size.width = result.size.width.max(row_width);
            result.size.height += row_height + self.spacing;
            result.minimum_height += row_minimum_height + self.spacing;
        }
        if !result.rows.is_empty() {
            result.size.height -= self.spacing;
            result.minimum_height -= self.spacing;
        }
        // A wrap is a column of atomic rows. Share a vertical shortage only
        // where those rows contain declared viewport capacity.
        if self.arrangement == Arrangement::Wrap && result.size.height > height + FIT_EPSILON {
            let gap = self.spacing * result.rows.len().saturating_sub(1) as f32;
            if gap > height {
                return Err(AllocationError::NoFit {
                    required: gap,
                    available: height,
                });
            }
            let capacities: Vec<_> = result
                .rows
                .iter()
                .map(|row| WidthCapacity {
                    preferred: row
                        .iter()
                        .map(|i| result.children[*i].geometry.size.height)
                        .fold(0.0, f32::max),
                    minimum: row
                        .iter()
                        .map(|i| result.children[*i].geometry.minimum_height)
                        .fold(0.0, f32::max),
                    weight: 1.0,
                })
                .collect();
            let heights =
                allocate_with_growth(height - gap, &capacities, &vec![0.0; capacities.len()])?;
            let mut y = 0.0;
            for (row, row_height) in result.rows.iter().zip(heights) {
                for i in row {
                    let placed = &mut result.children[*i];
                    if placed.geometry.size.height > row_height + FIT_EPSILON {
                        placed.geometry = self.children[placed.index].plan_width(
                            level,
                            renderer,
                            placed.geometry.size.width,
                            row_height,
                        )?;
                    }
                    placed.position.y = y + (row_height - placed.geometry.size.height) / 2.0;
                }
                y += row_height + self.spacing;
            }
            result.size.height = (y - self.spacing).max(0.0);
        }
        Ok(result)
    }
}

/// One exact level and component arrangement in the parent's ordered policy.
pub struct Stage<'a, Message> {
    pub level: Level,
    pub content: Child<'a, Message>,
}
impl<'a, Message> Stage<'a, Message> {
    pub fn new(level: Level, content: Child<'a, Message>) -> Self {
        Self { level, content }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StagePlan {
    pub stage_index: usize,
    pub level: Level,
    pub geometry: GeometryPlan,
}

/// No declared stage supplied complete fitting geometry. Nothing is implicitly
/// hidden, clipped, scrolled or promoted to a stronger undeclared level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoFit {
    pub attempted: usize,
}

pub fn choose_stage<'a, Message: 'a>(
    stages: &[Stage<'a, Message>],
    renderer: &Renderer,
    available: Size,
) -> Result<StagePlan, NoFit> {
    choose_stage_inner(stages, renderer, available)
}
fn choose_stage_inner<'a, Message: 'a>(
    stages: &[Stage<'a, Message>],
    renderer: &Renderer,
    available: Size,
) -> Result<StagePlan, NoFit> {
    for (stage_index, stage) in stages.iter().enumerate() {
        if let Ok(geometry) = stage.content.query_at(stage.level, renderer, available) {
            return Ok(StagePlan {
                stage_index,
                level: stage.level,
                geometry,
            });
        }
    }
    Err(NoFit {
        attempted: stages.len(),
    })
}

pub fn stages<'a, Message: 'a>(stages: Vec<Stage<'a, Message>>) -> ResponsiveLevels<'a, Message> {
    ResponsiveLevels {
        stages,
        width: Length::Fill,
        height: Length::Shrink,
        content: widget::space().width(0).height(0).into(),
        selection: None,
        fallback: None,
    }
}

/// Compatibility convenience: the same component at successive permitted levels.
pub fn responsive<'a, Message: 'a>(
    max_level: Level,
    view: impl Fn(Level) -> Element<'a, Message> + 'a,
) -> ResponsiveLevels<'a, Message> {
    responsive_child(max_level, child(view))
}

pub fn responsive_child<'a, Message: 'a>(
    max_level: Level,
    child: Child<'a, Message>,
) -> ResponsiveLevels<'a, Message> {
    stages(
        Level::ALL
            .into_iter()
            .take_while(|level| *level <= max_level)
            .map(|level| Stage::new(level, child.clone()))
            .collect(),
    )
}

pub fn row<'a, Message: 'a>(
    max_level: Level,
    children: Vec<Child<'a, Message>>,
    spacing: f32,
) -> ResponsiveLevels<'a, Message> {
    responsive_child(max_level, row_child(children, spacing))
}

pub struct ResponsiveLevels<'a, Message> {
    stages: Vec<Stage<'a, Message>>,
    width: Length,
    height: Length,
    content: Element<'a, Message>,
    selection: Option<Result<StagePlan, NoFit>>,
    fallback: Option<Box<dyn Fn() -> Element<'a, Message> + 'a>>,
}

impl<'a, Message: 'a> ResponsiveLevels<'a, Message> {
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }
    pub fn selection(&self) -> Option<&Result<StagePlan, NoFit>> {
        self.selection.as_ref()
    }
    pub fn fallback(mut self, fallback: impl Fn() -> Element<'a, Message> + 'a) -> Self {
        self.fallback = Some(Box::new(fallback));
        self
    }
}

struct VerifiedLeaf<'a, Message> {
    content: Element<'a, Message>,
    expected: Size,
    maximum: Size,
    compress_height: bool,
    agreement: Rc<Cell<bool>>,
}
impl<Message> Widget<Message, Theme, Renderer> for VerifiedLeaf<'_, Message> {
    forward_widget!(Message, Theme, Renderer; children, diff, size, draw, update, operate, mouse_interaction, overlay, drag_destinations);
    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, _: &layout::Limits) -> layout::Node {
        let node = self.content.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &layout::Limits::with_compression(
                Size::ZERO,
                self.maximum,
                Size::new(true, self.compress_height),
            ),
        );
        if !finite_size(node.size())
            || (node.size().width - self.expected.width).abs() > FIT_EPSILON
            || (node.size().height - self.expected.height).abs() > FIT_EPSILON
        {
            self.agreement.set(false);
        }
        node
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

// A standalone fixed-level view uses the same geometry implementation as stages.
struct FixedComponent<'a, Message> {
    child: Child<'a, Message>,
    level: Level,
    content: Element<'a, Message>,
}
impl<'a, Message: 'a> Widget<Message, Theme, Renderer> for FixedComponent<'a, Message> {
    forward_widget!(Message, Theme, Renderer; draw, update, operate, mouse_interaction, overlay, drag_destinations);
    fn diff(&mut self, _: &mut Tree) {}
    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let plan = self.child.query_at(self.level, renderer, limits.max());
        let agreement = Rc::new(Cell::new(true));
        self.content = match plan {
            Ok(plan) => self.child.render(self.level, &plan, agreement.clone()),
            Err(_) => widget::space().width(0).height(0).into(),
        };
        tree.diff_children(std::slice::from_mut(&mut self.content));
        let node = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        if agreement.get() {
            node
        } else {
            self.content = widget::space().width(0).height(0).into();
            tree.diff_children(std::slice::from_mut(&mut self.content));
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, limits)
        }
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
    measure_element_in(view, renderer, Size::INFINITE, true)
}

fn measure_element_in<'a, Message>(
    view: impl FnOnce() -> Element<'a, Message>,
    renderer: &Renderer,
    maximum: Size,
    compress_height: bool,
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
            &layout::Limits::with_compression(
                Size::ZERO,
                maximum,
                Size::new(true, compress_height),
            ),
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

impl<'view, Message: 'view> Widget<Message, Theme, Renderer> for ResponsiveLevels<'view, Message> {
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
        let mut rejected_live_tree = false;
        for (stage_index, stage) in self.stages.iter().enumerate() {
            let Ok(geometry) = stage.content.query_at(stage.level, renderer, available) else {
                continue;
            };
            if rejected_live_tree {
                // A failed native candidate may have a different arrangement.
                // Save named state before reconciling the next shape, just as
                // the native runtime does at a view rebuild boundary.
                let preserved = tree.take_all_named();
                tree::NAMED.with(|named| named.borrow_mut().extend(preserved));
            }
            let agreement = Rc::new(Cell::new(true));
            self.content = stage
                .content
                .render(stage.level, &geometry, agreement.clone());
            tree.diff_children(std::slice::from_mut(&mut self.content));
            // Ordinary native content always retains unbounded height here.
            // Finite-height clipping cannot certify a successful stage.
            let commit_limits = layout::Limits::with_compression(
                Size::ZERO,
                Size::new(geometry.size.width, f32::INFINITY),
                Size::new(true, true),
            );
            let node = self.content.as_widget_mut().layout(
                &mut tree.children[0],
                renderer,
                &commit_limits,
            );
            if agreement.get()
                && finite_size(node.size())
                && (node.size().width - geometry.size.width).abs() <= FIT_EPSILON
                && (node.size().height - geometry.size.height).abs() <= FIT_EPSILON
            {
                self.selection = Some(Ok(StagePlan {
                    stage_index,
                    level: stage.level,
                    geometry,
                }));
                return layout::Node::with_children(
                    limits.resolve(self.width, self.height, node.size()),
                    vec![node],
                );
            }
            // No hook can observe this intermediate candidate. Continue in the
            // parent's declared order, never adding a stronger implicit level.
            rejected_live_tree = true;
        }
        self.selection = Some(Err(NoFit {
            attempted: self.stages.len(),
        }));
        self.content = self.fallback.as_ref().map_or_else(
            || widget::space().width(0).height(0).into(),
            |fallback| fallback(),
        );
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

#[cfg(test)]
#[path = "layout_contract_tests.rs"]
mod contract_tests;

#[cfg(test)]
#[path = "layout_engine_tests.rs"]
mod engine_tests;
