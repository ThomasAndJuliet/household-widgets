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
use std::sync::Arc;

#[path = "layout_frame.rs"]
mod frame;
#[path = "layout_information.rs"]
mod information;
#[path = "layout_navigation.rs"]
mod navigation;
use frame::Frame;
pub use frame::framed_child;
pub use navigation::*;
#[path = "layout_presentation.rs"]
mod presentation;
pub use information::{
    AcceptedSourceReport, InformationId, InformationLoss, LossKind, SourceId, SourceReport,
    SourceReports,
};
use information::{SourceInformation, VerifiedBox};
pub use presentation::{
    Detail, Emphasis, Presentation, PresentationFamily, PresentationRequest,
    UnsupportedPresentation,
};

#[path = "layout_popup.rs"]
mod attached;
#[path = "layout_hover.rs"]
mod hover;
pub use attached::{
    Alignment, AnchorCandidate, AttachedPlan, Attachment, PlacementPreference, Side,
    choose_attached, commit_attached,
};
pub(crate) use attached::{
    ChainCard, ChainFailure, RetainedGeometry, choose_attached_avoiding, choose_attached_chain,
    commit_attached_retaining, commit_attached_retaining_avoiding,
};
pub use hover::choose_hover;
pub(crate) use hover::{HoverRetained, commit_hover_retaining};

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
#[derive(Clone, Debug)]
pub struct GeometryPlan {
    pub size: Size,
    pub hidden: bool,
    pub children: Vec<Placement>,
    // Row membership is part of the plan, not recomputed during commit.
    rows: Vec<Vec<usize>>,
    minimum_height: f32,
    native_size: Option<Size>,
    bindings: Vec<(PreparedId, Level)>,
    source_reports: Vec<SourceReport>,
}

// Geometric equality is useful when checking decorators and pure re-queries.
// Preparation bindings certify a commit, rather than changing its geometry.
impl PartialEq for GeometryPlan {
    fn eq(&self, other: &Self) -> bool {
        self.size == other.size
            && self.hidden == other.hidden
            && self.children == other.children
            && self.rows == other.rows
            && self.minimum_height == other.minimum_height
            && self.native_size == other.native_size
            && self.source_reports == other.source_reports
    }
}

// Keep the allocation alive in every plan, so pointer identity cannot be reused
// while an old plan still exists. Clones of an immutable preparation share it.
#[derive(Clone, Debug)]
struct PreparedId(Arc<()>);

impl PreparedId {
    fn new() -> Self {
        Self(Arc::new(()))
    }
}

impl PartialEq for PreparedId {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
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
            bindings: Vec::new(),
            source_reports: Vec::new(),
        }
    }
    fn hidden() -> Self {
        Self {
            hidden: true,
            ..Self::leaf(Size::ZERO)
        }
    }

    /// Trial facts about this plan and its immediate-child subplans. These are
    /// never active-source notifications; accepted native traversal publishes
    /// the current facts through `AcceptedSourceReport` instead.
    pub fn reports(&self) -> Vec<&SourceReport> {
        let mut reports: Vec<_> = self.source_reports.iter().collect();
        for child in &self.children {
            reports.extend(child.geometry.reports());
        }
        reports
    }

    pub(crate) fn same_preparation(&self, other: &Self) -> bool {
        self.bindings == other.bindings
            && self.children.len() == other.children.len()
            && self
                .children
                .iter()
                .zip(&other.children)
                .all(|(a, b)| a.index == b.index && a.geometry.same_preparation(&b.geometry))
    }
}

struct Composition<'a, Message> {
    children: Vec<Child<'a, Message>>,
    arrangement: Arrangement,
    spacing: f32,
}

type Forms<'a, Message> = Rc<Vec<(Level, Child<'a, Message>)>>;
type ViewMap<'a, Message> = Rc<dyn Fn(Element<'a, Message>) -> Element<'a, Message> + 'a>;
type MinimumQuery<'a> = Rc<dyn Fn(Level, &Renderer) -> Size + 'a>;

/// A queryable leaf or composite. Only the root selects a level. Composites
/// forward that exact level; outward weights never replace internal weights.
pub struct Child<'a, Message> {
    view: Rc<dyn Fn(Level) -> Element<'a, Message> + 'a>,
    query: SizeQuery<'a>,
    composition: Option<Rc<Composition<'a, Message>>>,
    frame: Option<Rc<Frame<'a, Message>>>,
    forms: Option<Forms<'a, Message>>,
    decorator: Option<ViewMap<'a, Message>>,
    hidden: Rc<dyn Fn(Level) -> bool + 'a>,
    viewport: Option<MinimumQuery<'a>>,
    weight: f32,
    grow: f32,
    grow_height: f32,
    preparation: PreparedId,
    presentation: Option<Presentation>,
    information: Option<SourceInformation<'a>>,
    regions: Vec<Region>,
}

impl<Message> Clone for Child<'_, Message> {
    fn clone(&self) -> Self {
        Self {
            view: self.view.clone(),
            query: self.query.clone(),
            composition: self.composition.clone(),
            frame: self.frame.clone(),
            forms: self.forms.clone(),
            decorator: self.decorator.clone(),
            hidden: self.hidden.clone(),
            viewport: self.viewport.clone(),
            weight: self.weight,
            grow: self.grow,
            grow_height: self.grow_height,
            preparation: self.preparation.clone(),
            presentation: self.presentation,
            information: self.information.clone(),
            regions: self.regions.clone(),
        }
    }
}

impl<'a, Message: 'a> Child<'a, Message> {
    fn refresh_preparation(&mut self) {
        self.preparation = PreparedId::new();
    }

    pub fn presentation(&self) -> Option<Presentation> {
        self.presentation
    }

    /// Report family-local information loss from prepared immutable data and
    /// this exact candidate geometry. This pure callback runs for trial queries
    /// too: it must not update a hover, registry, application state or perform
    /// work such as parsing, I/O or generation. Rebuild the Child on a content
    /// revision or presentation/dependency change; retain a clone only while
    /// its prepared snapshot is unchanged.
    pub fn report_information(
        mut self,
        source: SourceId,
        revision: u64,
        report: impl Fn(Level, &GeometryPlan) -> Vec<InformationLoss> + 'a,
    ) -> Self {
        self.information = Some(SourceInformation {
            source,
            revision,
            report: Rc::new(report),
        });
        self.refresh_preparation();
        self
    }

    /// Name an accepted native box for section navigation. Multiple roles may
    /// describe the same box. Rebuilding refreshes the preparation identity.
    pub fn report_region(mut self, region: Region) -> Self {
        self.regions.push(region);
        self.refresh_preparation();
        self
    }

    fn bind_geometry(&self, level: Level, mut plan: GeometryPlan) -> GeometryPlan {
        plan.bindings.push((self.preparation.clone(), level));
        if !plan.hidden
            && let Some(information) = &self.information
        {
            let losses = (information.report)(level, &plan);
            plan.source_reports.push(SourceReport {
                source: information.source,
                revision: information.revision,
                presentation: self.presentation,
                level,
                size: plan.size,
                losses,
                owner: self.preparation.clone(),
            });
        }
        plan
    }

    fn selected_form(&self, level: Level) -> Option<&Self> {
        self.forms.as_ref().map(|forms| select_form(forms, level))
    }

    fn has_composition(&self, level: Level) -> bool {
        self.composition.is_some()
            || self
                .frame
                .as_ref()
                .is_some_and(|frame| frame.content.has_composition(level))
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
            || self
                .frame
                .as_ref()
                .is_some_and(|frame| frame.content.is_hidden(level))
    }

    pub fn view(&self, level: Level) -> Element<'a, Message> {
        if self.is_hidden(level) {
            return widget::space().width(0).height(0).into();
        }
        // Preserve the native leaf's public tree identity for direct consumers
        // such as tab strips. Root stage commits use render/VerifiedLeaf; only
        // composites and explicitly decorated leaves need this fixed-level
        // allocation adapter here.
        if self.composition.is_none()
            && self.frame.is_none()
            && self.forms.is_none()
            && self.decorator.is_none()
            && self.presentation.is_none()
            && self.information.is_none()
        {
            return (self.view)(level);
        }
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
            return Ok(self.bind_geometry(level, GeometryPlan::hidden()));
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
            return Ok(self.bind_geometry(level, GeometryPlan::hidden()));
        }
        let mut plan = if let Some(form) = self.selected_form(level) {
            let mut selected = form.clone();
            if self.grow_height > 0.0 {
                selected.grow_height = self.grow_height;
            }
            selected.plan_width(level, renderer, width, height)?
        } else if let Some(composition) = &self.composition {
            composition.plan(level, renderer, width, height)?
        } else if let Some(frame) = &self.frame {
            frame.plan(level, renderer, width, height, self.grow_height)?
        } else {
            let viewport_height = self.viewport.as_ref().map(|minimum| {
                let minimum = minimum(level, renderer);
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
            if let Some(offered_height) = viewport_height
                && size.height > offered_height + FIT_EPSILON
            {
                return Err(AllocationError::NoFit {
                    required: size.height,
                    available: offered_height,
                });
            }
            // The parent reserves the allocated width even for shrink content.
            let mut plan = GeometryPlan::leaf(Size::new(width, size.height));
            plan.native_size = Some(size);
            if let Some(minimum) = &self.viewport {
                let minimum = minimum(level, renderer);
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
        Ok(self.bind_geometry(level, plan))
    }

    fn render(
        &self,
        level: Level,
        plan: &GeometryPlan,
        agreement: Rc<Cell<bool>>,
    ) -> Element<'a, Message> {
        let binding_matches = plan
            .bindings
            .iter()
            .any(|(id, planned_level)| id == &self.preparation && *planned_level == level);
        let content = if !binding_matches || plan.hidden {
            widget::space().width(0).height(0).into()
        } else {
            self.render_content(level, plan, agreement.clone())
        };
        let content = match &self.decorator {
            Some(_) if !binding_matches || plan.hidden => content,
            Some(decorate) => {
                let content = Element::new(VerifiedDecoration {
                    content,
                    expected: plan.size,
                    agreement: agreement.clone(),
                    check_constraints: true,
                });
                Element::new(VerifiedDecoration {
                    content: decorate(content),
                    expected: plan.size,
                    agreement: agreement.clone(),
                    check_constraints: false,
                })
            }
            None => content,
        };
        Element::new(VerifiedBox {
            content,
            reports: if binding_matches && !plan.hidden {
                plan.source_reports
                    .iter()
                    .filter(|report| report.owner == self.preparation)
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            },
            regions: if binding_matches && !plan.hidden {
                self.regions.clone()
            } else {
                Vec::new()
            },
            binding_matches,
            agreement,
        })
    }

    fn render_content(
        &self,
        level: Level,
        plan: &GeometryPlan,
        agreement: Rc<Cell<bool>>,
    ) -> Element<'a, Message> {
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
        if let Some(frame) = &self.frame {
            return frame.render(level, plan, agreement, self.grow_height);
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
        // The plan has already omitted hidden forms. Native `with_children`
        // filters zero-sized hints, which would discard valid zero-height
        // horizontal spacers (or zero-width vertical slots). Preserve every
        // explicitly allocated slot with `from_vec` instead.
        let make_row = |indices: &[usize]| -> Element<'a, Message> {
            widget::Row::from_vec(
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
            Arrangement::Wrap => widget::Column::from_vec(
                plan.rows
                    .iter()
                    .map(|row| make_row(row))
                    .collect::<Vec<_>>(),
            )
            .spacing(composition.spacing)
            .into(),
            Arrangement::Column => widget::Column::from_vec(
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

    /// Decorate this component's rendered content without changing its geometry.
    /// Useful ranges, structured forms and exact-level queries stay unchanged.
    /// Apply only styling or alignment inside the existing component box: no
    /// padding, size changes or overflow. Native commits verify the decorated
    /// size and reject a disagreeing stage. Factories and decorators must be
    /// free of application side effects. Hidden forms are never decorated.
    pub fn map_view(
        mut self,
        decorate: impl Fn(Element<'a, Message>) -> Element<'a, Message> + 'a,
    ) -> Self {
        self.decorator = Some(match self.decorator.take() {
            Some(previous) => Rc::new(move |content| decorate(previous(content))),
            None => Rc::new(decorate),
        });
        self.refresh_preparation();
        self
    }

    pub fn weight(mut self, weight: f32) -> Self {
        assert!(weight.is_finite() && weight >= 0.0);
        self.weight = weight;
        self.refresh_preparation();
        self
    }

    /// Share surplus only after shrinking has finished.
    pub fn grow(mut self, grow: f32) -> Self {
        assert!(grow.is_finite() && grow >= 0.0);
        self.grow = grow;
        self.refresh_preparation();
        self
    }

    /// Share surplus height in a column independently of width growth. Native
    /// content keeps its measured height floor; a declared viewport expands.
    pub fn grow_height(mut self, grow: f32) -> Self {
        assert!(grow.is_finite() && grow >= 0.0);
        self.grow_height = grow;
        self.refresh_preparation();
        self
    }

    /// Explicit component permission to omit this form. No layout slot, gap,
    /// event target, overlay or accessibility node is built for hidden children.
    pub fn hidden_when(mut self, hidden: impl Fn(Level) -> bool + 'a) -> Self {
        self.hidden = Rc::new(hidden);
        self.refresh_preparation();
        self
    }

    pub fn round_width_up(mut self) -> Self {
        self.refresh_preparation();
        let query = self.query.clone();
        self.query = Rc::new(move |level, renderer| {
            let mut measured = query(level, renderer);
            measured.preferred.width = measured.preferred.width.ceil();
            measured.minimum.width = measured.minimum.width.ceil();
            measured
        });
        self
    }

    /// Set the desired width independently of the measured useful minimum.
    /// A parent may allocate less down to that minimum; when the minimum is
    /// larger than the requested width it remains authoritative. Geometry is
    /// remeasured at the allocated width, including wrapping and chrome.
    pub fn preferred_width(self, width: f32) -> Self {
        self.preferred_width_with(move |_, _, _| width)
    }

    /// Derive a preferred width from the same prepared data and renderer as
    /// the component, without replacing its independently measured minimum.
    pub fn preferred_width_with(
        mut self,
        preferred: impl Fn(Level, &Renderer, MeasuredSize) -> f32 + 'a,
    ) -> Self {
        self.refresh_preparation();
        let query = self.query.clone();
        self.query = Rc::new(move |level, renderer| {
            let mut measured = query(level, renderer);
            let width = preferred(level, renderer, measured);
            measured.preferred.width = if width.is_finite() && width >= 0.0 {
                width.max(measured.minimum.width)
            } else {
                f32::NAN
            };
            measured
        });
        self
    }

    /// Declare a finite, nonnegative useful width floor. If native intrinsic
    /// width is smaller (including an all-Fill subtree reporting zero), the
    /// preferred offer is raised to the floor. A native leaf with positive
    /// infinite intrinsic width is probed at this declared floor; only a finite
    /// fitting native result can use the floor as its initial preferred offer.
    /// Native content is measured again
    /// at its allocated width, including the actual height of wrapping text.
    /// The component is responsible for making that form usable.
    pub fn minimum_width(self, minimum: impl Fn(Level, Size) -> f32 + 'a) -> Self {
        self.minimum_width_with(move |level, _, preferred| minimum(level, preferred))
    }

    /// A useful width floor measured from prepared native content using the
    /// query's renderer. The callback is pure and must capture the same data,
    /// typography and presentation as this component's selected view.
    pub fn minimum_width_with(
        mut self,
        minimum: impl Fn(Level, &Renderer, Size) -> f32 + 'a,
    ) -> Self {
        self.refresh_preparation();
        let query = self.query.clone();
        let native = if self.composition.is_none() && self.forms.is_none() && self.frame.is_none() {
            Some(self.view.clone())
        } else {
            None
        };
        self.query = Rc::new(move |level, renderer| {
            let mut measured = query(level, renderer);
            let recoverable = measured.preferred.width == f32::INFINITY
                && measured.preferred.height.is_finite()
                && measured.preferred.height >= 0.0
                && measured.minimum.height.is_finite()
                && measured.minimum.height >= 0.0
                && native.is_some();
            if (measured.preferred.width.is_finite() && measured.preferred.width >= 0.0)
                || recoverable
            {
                let floor = minimum(level, renderer, measured.preferred);
                if floor.is_finite() && floor >= 0.0 {
                    if recoverable {
                        // Some native Fill controls have no intrinsic width.
                        // The component's declared floor is the finite offer;
                        // never substitute an arbitrary window or probe width.
                        let bounded = measure_element_in(
                            || native.as_ref().expect("native leaf factory")(level),
                            renderer,
                            Size::new(floor, f32::INFINITY),
                            true,
                        );
                        if finite_size(bounded) && bounded.width <= floor + FIT_EPSILON {
                            measured.preferred = Size::new(floor, bounded.height);
                            measured.minimum.width = floor;
                            measured.minimum.height = measured.minimum.height.min(bounded.height);
                        } else {
                            measured.minimum.width = f32::NAN;
                        }
                    } else {
                        // An all-Fill native subtree can report intrinsic zero.
                        measured.minimum.width = floor;
                        measured.preferred.width = measured.preferred.width.max(floor);
                    }
                } else {
                    measured.minimum.width = f32::NAN;
                }
            } else {
                measured.minimum.width = f32::NAN;
            }
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
        frame: None,
        forms: None,
        decorator: None,
        hidden: Rc::new(|_| false),
        viewport: None,
        weight: 1.0,
        grow: 0.0,
        grow_height: 0.0,
        preparation: PreparedId::new(),
        presentation: None,
        information: None,
        regions: Vec::new(),
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
        frame: None,
        forms: Some(alternatives),
        decorator: None,
        hidden: Rc::new(|_| false),
        viewport: None,
        weight: 1.0,
        grow: 0.0,
        grow_height: 0.0,
        preparation: PreparedId::new(),
        presentation: None,
        information: None,
        regions: Vec::new(),
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
/// If a Fill viewport has no finite intrinsic preferred dimension, its declared
/// minimum supplies that initial offer after a bounded native probe succeeds.
pub fn scroll_child<'a, Message: 'a>(
    view: impl Fn(Level) -> Element<'a, Message> + 'a,
    minimum: Size,
) -> Child<'a, Message> {
    assert!(finite_size(minimum));
    scroll_child_with_minimum(view, move |_, _| minimum)
}

/// Declare a scrolling viewport whose useful floor is measured with the same
/// native renderer as its contents. A changed level or typography therefore
/// refreshes both measurement and selected viewport geometry. Nonfinite or
/// negative queried floors are explicit invalid input, never clipped viewports.
pub fn scroll_child_with_minimum<'a, Message: 'a>(
    view: impl Fn(Level) -> Element<'a, Message> + 'a,
    minimum: impl Fn(Level, &Renderer) -> Size + 'a,
) -> Child<'a, Message> {
    let minimum: MinimumQuery<'a> = Rc::new(minimum);
    let viewport = minimum.clone();
    let mut child = child(view);
    let query = child.query.clone();
    let native = child.view.clone();
    child.query = Rc::new(move |level, renderer| {
        let minimum = minimum(level, renderer);
        let measured = query(level, renderer);
        if !finite_size(minimum) {
            return MeasuredSize {
                preferred: Size::new(f32::NAN, f32::NAN),
                minimum,
            };
        }
        let invalid = |value: f32| value.is_nan() || value < 0.0;
        if invalid(measured.preferred.width) || invalid(measured.preferred.height) {
            return MeasuredSize {
                preferred: measured.preferred,
                minimum,
            };
        }
        let mut preferred = measured.preferred;
        if preferred.width.is_infinite() || preferred.height.is_infinite() {
            // This component explicitly declares a scrolling viewport. Validate
            // its whole native box, including controls, at that finite minimum.
            let bounded = measure_element_in(|| native(level), renderer, minimum, false);
            if !finite_size(bounded)
                || bounded.width > minimum.width + FIT_EPSILON
                || bounded.height > minimum.height + FIT_EPSILON
            {
                return MeasuredSize {
                    preferred: Size::new(f32::NAN, f32::NAN),
                    minimum,
                };
            }
            if preferred.width.is_infinite() {
                preferred.width = minimum.width;
            }
            if preferred.height.is_infinite() {
                preferred.height = minimum.height;
            }
        }
        MeasuredSize {
            preferred: Size::new(
                preferred.width.max(minimum.width),
                preferred.height.max(minimum.height),
            ),
            minimum,
        }
    });
    child.viewport = Some(viewport);
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
        self.refresh_preparation();
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
        frame: None,
        forms: None,
        decorator: None,
        hidden: Rc::new(|_| false),
        viewport: None,
        weight: 1.0,
        grow: 0.0,
        grow_height: 0.0,
        preparation: PreparedId::new(),
        presentation: None,
        information: None,
        regions: Vec::new(),
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

// A component decorator may contain a whole nested composition. Validate its
// own box before the parent's fixed slot could conceal a changed requirement.
struct VerifiedDecoration<'a, Message> {
    content: Element<'a, Message>,
    expected: Size,
    agreement: Rc<Cell<bool>>,
    check_constraints: bool,
}
impl<Message> Widget<Message, Theme, Renderer> for VerifiedDecoration<'_, Message> {
    forward_widget!(Message, Theme, Renderer; children, diff, size, draw, update, operate, mouse_interaction, overlay, drag_destinations);
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = if self.check_constraints {
            // Padding or a smaller explicit size must not conceal overflow by
            // reducing the raw component's already allocated box.
            if limits.max().width + FIT_EPSILON < self.expected.width
                || limits.max().height + FIT_EPSILON < self.expected.height
            {
                self.agreement.set(false);
            }
            *limits
        } else {
            layout::Limits::with_compression(
                Size::ZERO,
                Size::new(self.expected.width, f32::INFINITY),
                Size::new(true, true),
            )
        };
        let node = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &limits);
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
                Size::new(self.maximum.width.is_infinite(), self.compress_height),
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

pub(crate) fn isolate_named_state() -> impl Drop {
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
    // Only an unbounded intrinsic probe compresses Fill widths. At an
    // allocated finite width, native Fill columns must consume that offer.
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
                Size::new(maximum.width.is_infinite(), compress_height),
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

#[cfg(test)]
#[path = "layout_decorator_tests.rs"]
mod decorator_tests;

#[cfg(test)]
#[path = "layout_input_tests.rs"]
mod input_tests;

#[cfg(test)]
#[path = "layout_presentation_tests.rs"]
mod presentation_tests;

#[cfg(test)]
mod renderer_minimum_tests {
    use super::*;
    use cosmic::iced::advanced::renderer::Headless;
    use cosmic::iced::{Font, Pixels};

    fn renderer() -> Renderer {
        cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
            Font::default(),
            Pixels(14.0),
            Some("tiny-skia"),
        ))
        .expect("headless tiny-skia renderer")
    }

    fn floor(size: f32) -> Child<'static, ()> {
        child(move |_| {
            widget::container(widget::text("More models").size(size))
                .padding([4.0, 12.0])
                .into()
        })
    }

    #[test]
    fn useful_width_uses_actual_native_typography_in_query_and_commit() {
        let renderer = renderer();
        let small = floor(12.0);
        let large = floor(24.0);
        let small_width = small.query(Level::Preferred, &renderer).preferred.width;
        let large_width = large.query(Level::Preferred, &renderer).preferred.width;
        assert!(
            large_width > small_width,
            "the native font determines the useful floor"
        );
        let content: Child<'_, ()> = child(|_| {
            widget::text("A wrapping model label that stays complete")
                .width(Length::Fill)
                .into()
        })
        .minimum_width_with(move |level, renderer, _| {
            if level >= Level::Mild {
                large.query(level, renderer).preferred.width
            } else {
                small.query(level, renderer).preferred.width
            }
        });
        for (level, width) in [(Level::Preferred, small_width), (Level::Mild, large_width)] {
            assert_eq!(content.query(level, &renderer).minimum.width, width);
            assert!(
                content
                    .query_at(level, &renderer, Size::new(width - 1.0, 1000.0))
                    .is_err()
            );
            let mut selected = stages(vec![Stage::new(level, content.clone())]);
            let mut tree = Tree::empty();
            let node = selected.layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, Size::new(width, 1000.0)),
            );
            assert!(
                selected.selection().unwrap().is_ok(),
                "native commit must verify the measured floor"
            );
            assert_eq!(node.size().width, width);
        }
    }

    #[test]
    fn scrolling_floor_refreshes_with_native_typography_and_rejects_small_offers() {
        let renderer = renderer();
        for font_size in [12.0, 24.0] {
            let probe = floor(font_size);
            let minimum = probe.query(Level::Preferred, &renderer).preferred;
            let content: Child<'_, ()> = scroll_child_with_minimum(
                move |_| {
                    widget::scrollable(widget::text("A model row\n".repeat(50)).size(font_size))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                },
                move |level, renderer| probe.query(level, renderer).preferred,
            );
            assert_eq!(content.query(Level::Preferred, &renderer).minimum, minimum);
            assert!(
                content
                    .query_at(
                        Level::Preferred,
                        &renderer,
                        Size::new(minimum.width - 1.0, 100.0)
                    )
                    .is_err()
            );
            assert!(
                content
                    .query_at(
                        Level::Preferred,
                        &renderer,
                        Size::new(minimum.width, minimum.height - 1.0)
                    )
                    .is_err()
            );
            let mut selected = stages(vec![Stage::new(Level::Preferred, content)]);
            let mut tree = Tree::empty();
            let node = selected.layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, minimum),
            );
            assert!(selected.selection().unwrap().is_ok());
            assert_eq!(node.size(), minimum);
        }
    }

    #[test]
    fn invalid_measured_floors_do_not_produce_a_plan() {
        let renderer = renderer();
        for invalid in [f32::NAN, f32::INFINITY, -1.0] {
            let width: Child<'_, ()> =
                child(|_| widget::text("Model").into()).minimum_width_with(move |_, _, _| invalid);
            assert_eq!(
                width.query_at(Level::Preferred, &renderer, Size::new(300.0, 300.0)),
                Err(AllocationError::InvalidInput)
            );
            let scrolling: Child<'_, ()> = scroll_child_with_minimum(
                |_| widget::scrollable(widget::text("Model")).into(),
                move |_, _| Size::new(invalid, 32.0),
            );
            assert_eq!(
                scrolling.query_at(Level::Preferred, &renderer, Size::new(300.0, 300.0)),
                Err(AllocationError::InvalidInput)
            );
        }
    }
}
