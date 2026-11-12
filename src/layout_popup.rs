// SPDX-License-Identifier: GPL-3.0-only

//! Attached geometry is a query and commit extension of the staged engine.
//! A candidate offers a real region before measurement. Nothing is moved back
//! into the window after measurement, and scrolling remains an explicit Child.

use super::*;
use cosmic::iced::Point;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Below,
    Above,
    Right,
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    Start,
    End,
    Center,
    /// Prefer the source's start, bounded by the entire offered region. This
    /// equation participates in measurement and never changes the primary side.
    StartBounded,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorCandidate {
    pub side: Side,
    pub alignment: Alignment,
    pub gap: f32,
}

impl AnchorCandidate {
    pub fn new(side: Side, alignment: Alignment) -> Self {
        Self {
            side,
            alignment,
            gap: 0.0,
        }
    }
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }
}

/// Callers choose which preference is stronger; list and panel policies need
/// not share an ordering. Each stage still forwards one exact shared Level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlacementPreference {
    #[default]
    RepresentationFirst,
    AnchorFirst,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    pub anchor: Rectangle,
    pub candidates: Vec<AnchorCandidate>,
    pub preference: PlacementPreference,
    /// Expand to the candidate region's width instead of the measured
    /// preferred panel width. Growth inside that width remains the Child's.
    pub fill_width: bool,
    /// Insets between the complete measured paint box and its visible surface.
    /// Attachment uses surface edges; the whole paint box must still fit.
    pub surface_insets: cosmic::iced::Padding,
}

impl Attachment {
    pub fn new(anchor: Rectangle, candidates: Vec<AnchorCandidate>) -> Self {
        Self {
            anchor,
            candidates,
            preference: PlacementPreference::default(),
            fill_width: false,
            surface_insets: cosmic::iced::Padding::ZERO,
        }
    }
    /// Below/above candidates are useful for a horizontal list's child panel.
    pub fn below(anchor: Rectangle) -> Self {
        Self::new(
            anchor,
            vec![
                AnchorCandidate::new(Side::Below, Alignment::Start),
                AnchorCandidate::new(Side::Below, Alignment::End),
                AnchorCandidate::new(Side::Above, Alignment::Start),
                AnchorCandidate::new(Side::Above, Alignment::End),
            ],
        )
    }
    pub fn beside(anchor: Rectangle) -> Self {
        Self::new(
            anchor,
            vec![
                AnchorCandidate::new(Side::Right, Alignment::Start),
                AnchorCandidate::new(Side::Left, Alignment::Start),
                AnchorCandidate::new(Side::Right, Alignment::End),
                AnchorCandidate::new(Side::Left, Alignment::End),
            ],
        )
    }
    pub fn preference(mut self, preference: PlacementPreference) -> Self {
        self.preference = preference;
        self
    }
    pub fn fill_width(mut self) -> Self {
        self.fill_width = true;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AttachedPlan {
    pub stage: StagePlan,
    pub candidate_index: usize,
    pub bounds: Rectangle,
}

/// The offer and the origin are derived from the same anchor equation. A
/// candidate is rejected when its complete queried bounds do not fit.
pub fn choose_attached<'a, Message: 'a>(
    stages: &[Stage<'a, Message>],
    renderer: &Renderer,
    window: Rectangle,
    attachment: &Attachment,
) -> Result<AttachedPlan, NoFit> {
    choose_attached_inner(stages, renderer, window, attachment, &[], &[])
}

fn choose_attached_inner<'a, Message: 'a>(
    stages: &[Stage<'a, Message>],
    renderer: &Renderer,
    window: Rectangle,
    attachment: &Attachment,
    rejected: &[(usize, usize)],
    obstacles: &[Rectangle],
) -> Result<AttachedPlan, NoFit> {
    let attempted = stages.len().saturating_mul(attachment.candidates.len());
    if !valid_rect(window) || !valid_rect(attachment.anchor) {
        return Err(NoFit { attempted });
    }
    let query = |stage_index: usize, candidate_index: usize| {
        if rejected.contains(&(stage_index, candidate_index)) {
            return None;
        }
        let candidate = attachment.candidates[candidate_index];
        let anchor = paint_anchor(attachment.anchor, candidate.side, attachment.surface_insets);
        let mut available = offer(window, anchor, candidate)?;
        let stage = &stages[stage_index];
        if !attachment.fill_width {
            available.width = available
                .width
                .min(stage.content.query(stage.level, renderer).preferred.width);
        }
        let geometry = stage
            .content
            .query_at(stage.level, renderer, available)
            .ok()?;
        let position = origin(window, anchor, candidate, geometry.size);
        let bounds = Rectangle::new(position, geometry.size);
        (contains(window, bounds)
            && !obstacles.iter().any(|obstacle| {
                bounds
                    .intersection(obstacle)
                    .is_some_and(|area| area.width > 0.0 && area.height > 0.0)
            }))
        .then_some(AttachedPlan {
            stage: StagePlan {
                stage_index,
                level: stage.level,
                geometry,
            },
            candidate_index,
            bounds,
        })
    };
    match attachment.preference {
        PlacementPreference::RepresentationFirst => {
            for stage in 0..stages.len() {
                for candidate in 0..attachment.candidates.len() {
                    if let Some(plan) = query(stage, candidate) {
                        return Ok(plan);
                    }
                }
            }
        }
        PlacementPreference::AnchorFirst => {
            for candidate in 0..attachment.candidates.len() {
                for stage in 0..stages.len() {
                    if let Some(plan) = query(stage, candidate) {
                        return Ok(plan);
                    }
                }
            }
        }
    }
    Err(NoFit { attempted })
}

/// Query a permitted attachment while protecting earlier ancestor surfaces.
/// The immediate parent may share its reserved shadow margin; older ancestors
/// are obstacles, so reversing a nested side placement cannot cover them.
pub(crate) fn choose_attached_avoiding<'a, Message: 'a>(
    stages: &[Stage<'a, Message>],
    renderer: &Renderer,
    window: Rectangle,
    attachment: &Attachment,
    obstacles: &[Rectangle],
) -> Result<AttachedPlan, NoFit> {
    choose_attached_inner(stages, renderer, window, attachment, &[], obstacles)
}

/// One physical box can embed several consecutive logical chain levels. The
/// component supplies every composition; the engine knows only their spans.
#[derive(Clone)]
pub(crate) struct ChainCard {
    pub index: usize,
    pub span: usize,
    pub plan: AttachedPlan,
}

pub(crate) enum ChainFailure {
    MissingSource(NoFit),
    NoFit(NoFit),
}

pub(crate) struct ChainSelection {
    pub cards: Vec<ChainCard>,
    pub no_fit: Option<(usize, NoFit)>,
}

/// Negotiate ordinary attachments before a component-declared embedded form.
/// Queries receive the accepted prefix, allowing a native adapter to resolve
/// descendant anchors in a disposable transaction. No runtime state belongs to
/// this scheduler, and it never enumerates a descendant's internal forms.
pub(crate) fn choose_attached_chain(
    count: usize,
    max_span: impl Fn(usize) -> usize,
    mut query: impl FnMut(&[ChainCard], usize, usize) -> Result<AttachedPlan, ChainFailure>,
) -> ChainSelection {
    let mut cards: Vec<ChainCard> = Vec::new();
    let mut spans = vec![1; count];
    let mut index = 0;
    while index < count {
        let span = spans[index];
        match query(&cards, index, span) {
            Ok(plan) => {
                cards.push(ChainCard { index, span, plan });
                index += span;
            }
            Err(failure) => {
                let (error, can_embed) = match failure {
                    ChainFailure::MissingSource(error) => (error, false),
                    ChainFailure::NoFit(error) => (error, true),
                };
                if can_embed && let Some(parent) = cards.last() {
                    let combined = parent.span + span;
                    if combined <= max_span(parent.index) {
                        index = parent.index;
                        spans[index] = combined;
                        cards.pop();
                        continue;
                    }
                }
                return ChainSelection {
                    cards,
                    no_fit: Some((index, error)),
                };
            }
        }
    }
    ChainSelection {
        cards,
        no_fit: None,
    }
}

/// Select and commit in the declared order, trying the next pair when live
/// native state disagrees with a disposable query. This mirrors `stages`.
pub fn commit_attached<'a, Message: 'a>(
    stages: &[Stage<'a, Message>],
    renderer: &Renderer,
    window: Rectangle,
    attachment: &Attachment,
    content: &mut Element<'a, Message>,
    tree: &mut Tree,
) -> Result<(AttachedPlan, layout::Node), NoFit> {
    commit_attached_retaining(
        stages,
        renderer,
        window,
        attachment,
        content,
        tree,
        &mut RetainedGeometry::default(),
    )
}

/// A selected native widget object survives repeated overlay layout within a
/// view generation. Native controls can keep redraw status on that object as
/// well as durable state in Tree. A view diff explicitly invalidates this.
pub(crate) struct RetainedGeometry {
    plan: Option<AttachedPlan>,
    agreement: Rc<Cell<bool>>,
}
impl Default for RetainedGeometry {
    fn default() -> Self {
        Self {
            plan: None,
            agreement: Rc::new(Cell::new(true)),
        }
    }
}
impl RetainedGeometry {
    pub(crate) fn clear(&mut self) {
        self.plan = None;
    }
}

pub(crate) fn commit_attached_retaining<'a, Message: 'a>(
    stages: &[Stage<'a, Message>],
    renderer: &Renderer,
    window: Rectangle,
    attachment: &Attachment,
    content: &mut Element<'a, Message>,
    tree: &mut Tree,
    retained: &mut RetainedGeometry,
) -> Result<(AttachedPlan, layout::Node), NoFit> {
    commit_attached_retaining_avoiding(
        stages,
        renderer,
        window,
        attachment,
        content,
        tree,
        retained,
        &[],
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn commit_attached_retaining_avoiding<'a, Message: 'a>(
    stages: &[Stage<'a, Message>],
    renderer: &Renderer,
    window: Rectangle,
    attachment: &Attachment,
    content: &mut Element<'a, Message>,
    tree: &mut Tree,
    retained: &mut RetainedGeometry,
    obstacles: &[Rectangle],
) -> Result<(AttachedPlan, layout::Node), NoFit> {
    let mut rejected = Vec::new();
    loop {
        let plan =
            choose_attached_inner(stages, renderer, window, attachment, &rejected, obstacles)?;
        if !rejected.is_empty() {
            let preserved = tree.take_all_named();
            tree::NAMED.with(|named| named.borrow_mut().extend(preserved));
        }
        let result = if retained.plan.as_ref().is_some_and(|previous| {
            previous == &plan
                && previous
                    .stage
                    .geometry
                    .same_preparation(&plan.stage.geometry)
        }) {
            plan.layout_committed(renderer, content, tree, &retained.agreement)
        } else {
            plan.commit_with_agreement(stages, renderer, content, tree, retained.agreement.clone())
        };
        match result {
            Ok(node) => {
                retained.plan = Some(plan.clone());
                return Ok((plan, node));
            }
            Err(_) => {
                retained.clear();
                rejected.push((plan.stage.stage_index, plan.candidate_index));
            }
        }
    }
}

impl AttachedPlan {
    /// Commit the exact queried GeometryPlan with ordinary native tree diffing.
    /// The caller retains `content` and `tree` between layouts, preserving
    /// compatible native widget state. A disagreeing native layout is NoFit.
    pub fn commit<'a, Message: 'a>(
        &self,
        stages: &[Stage<'a, Message>],
        renderer: &Renderer,
        content: &mut Element<'a, Message>,
        tree: &mut Tree,
    ) -> Result<layout::Node, NoFit> {
        self.commit_with_agreement(stages, renderer, content, tree, Rc::new(Cell::new(true)))
    }
    fn commit_with_agreement<'a, Message: 'a>(
        &self,
        stages: &[Stage<'a, Message>],
        renderer: &Renderer,
        content: &mut Element<'a, Message>,
        tree: &mut Tree,
        agreement: Rc<Cell<bool>>,
    ) -> Result<layout::Node, NoFit> {
        let failure = NoFit { attempted: 1 };
        let Some(stage) = stages.get(self.stage.stage_index) else {
            return Err(failure);
        };
        if stage.level != self.stage.level {
            return Err(failure);
        }
        *content = stage
            .content
            .render(stage.level, &self.stage.geometry, agreement.clone());
        tree.diff(content.as_widget_mut());
        self.layout_committed(renderer, content, tree, &agreement)
    }
    fn layout_committed<Message>(
        &self,
        renderer: &Renderer,
        content: &mut Element<'_, Message>,
        tree: &mut Tree,
        agreement: &Cell<bool>,
    ) -> Result<layout::Node, NoFit> {
        agreement.set(true);
        let node = content.as_widget_mut().layout(
            tree,
            renderer,
            &layout::Limits::with_compression(
                Size::ZERO,
                Size::new(self.stage.geometry.size.width, f32::INFINITY),
                Size::new(true, true),
            ),
        );
        if !agreement.get()
            || !finite_size(node.size())
            || (node.size().width - self.bounds.width).abs() > FIT_EPSILON
            || (node.size().height - self.bounds.height).abs() > FIT_EPSILON
        {
            return Err(NoFit { attempted: 1 });
        }
        Ok(node.move_to(self.bounds.position()))
    }
}

fn valid_rect(rect: Rectangle) -> bool {
    rect.x.is_finite()
        && rect.y.is_finite()
        && finite_size(rect.size())
        && rect.width >= 0.0
        && rect.height >= 0.0
}
fn contains(window: Rectangle, rect: Rectangle) -> bool {
    rect.x >= window.x
        && rect.y >= window.y
        && rect.x + rect.width <= window.x + window.width
        && rect.y + rect.height <= window.y + window.height
}
// Convert the visible attachment equation to the full paint-box equation.
// This does not reposition a committed plan: both offer and origin use the
// same inset-aware anchor before the content is measured.
fn paint_anchor(mut anchor: Rectangle, side: Side, insets: cosmic::iced::Padding) -> Rectangle {
    match side {
        Side::Right | Side::Left => {
            anchor.y -= insets.top;
            anchor.height += insets.top + insets.bottom;
            if side == Side::Right {
                anchor.width -= insets.left;
            } else {
                anchor.x += insets.right;
                anchor.width -= insets.right;
            }
        }
        Side::Below | Side::Above => {
            anchor.x -= insets.left;
            anchor.width += insets.left + insets.right;
            if side == Side::Below {
                anchor.height -= insets.top;
            } else {
                anchor.y += insets.bottom;
                anchor.height -= insets.bottom;
            }
        }
    }
    anchor
}

fn offer(window: Rectangle, anchor: Rectangle, candidate: AnchorCandidate) -> Option<Size> {
    if !candidate.gap.is_finite() || candidate.gap < 0.0 {
        return None;
    }
    let vertical = matches!(candidate.side, Side::Above | Side::Below);
    let (a_start, a_end, w_start, w_end) = if vertical {
        (
            anchor.x,
            anchor.x + anchor.width,
            window.x,
            window.x + window.width,
        )
    } else {
        (
            anchor.y,
            anchor.y + anchor.height,
            window.y,
            window.y + window.height,
        )
    };
    let cross = match candidate.alignment {
        Alignment::Start => w_end - a_start,
        Alignment::End => a_end - w_start,
        Alignment::Center => {
            let center = (a_start + a_end) / 2.0;
            2.0 * (center - w_start).min(w_end - center)
        }
        Alignment::StartBounded => w_end - w_start,
    };
    let main = match candidate.side {
        Side::Below => window.y + window.height - anchor.y - anchor.height - candidate.gap,
        Side::Above => anchor.y - window.y - candidate.gap,
        Side::Right => window.x + window.width - anchor.x - anchor.width - candidate.gap,
        Side::Left => anchor.x - window.x - candidate.gap,
    };
    (cross >= 0.0 && main >= 0.0).then_some(if vertical {
        Size::new(cross, main)
    } else {
        Size::new(main, cross)
    })
}
fn origin(window: Rectangle, anchor: Rectangle, candidate: AnchorCandidate, size: Size) -> Point {
    let align = |start: f32, length: f32, actual: f32, region_start: f32, region_length: f32| {
        match candidate.alignment {
            Alignment::Start => start,
            Alignment::End => start + length - actual,
            Alignment::Center => start + (length - actual) / 2.0,
            Alignment::StartBounded => start
                .max(region_start)
                .min(region_start + region_length - actual),
        }
    };
    match candidate.side {
        Side::Below => Point::new(
            align(anchor.x, anchor.width, size.width, window.x, window.width),
            anchor.y + anchor.height + candidate.gap,
        ),
        Side::Above => Point::new(
            align(anchor.x, anchor.width, size.width, window.x, window.width),
            anchor.y - size.height - candidate.gap,
        ),
        Side::Right => Point::new(
            anchor.x + anchor.width + candidate.gap,
            align(
                anchor.y,
                anchor.height,
                size.height,
                window.y,
                window.height,
            ),
        ),
        Side::Left => Point::new(
            anchor.x - size.width - candidate.gap,
            align(
                anchor.y,
                anchor.height,
                size.height,
                window.y,
                window.height,
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
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
    fn box_child(width: f32, height: f32) -> Child<'static, ()> {
        child(move |_| widget::space().width(width).height(height).into())
    }

    #[test]
    fn attached_geometry_flips_before_commit_and_remains_inside_resized_windows() {
        let renderer = renderer();
        let stages = vec![Stage::new(Level::Preferred, box_child(120.0, 80.0))];
        for window_size in [Size::new(400.0, 300.0), Size::new(200.0, 160.0)] {
            let window = Rectangle::with_size(window_size);
            let anchor = Rectangle::new(
                Point::new(window_size.width - 30.0, window_size.height - 20.0),
                Size::new(20.0, 10.0),
            );
            let attachment = Attachment::below(anchor);
            let expected = choose_attached(&stages, &renderer, window, &attachment).unwrap();
            assert_eq!(expected.candidate_index, 3, "above-end fits; no post-clamp");
            assert!(contains(window, expected.bounds));
            let mut content = widget::space().into();
            let (committed, node) = commit_attached(
                &stages,
                &renderer,
                window,
                &attachment,
                &mut content,
                &mut Tree::empty(),
            )
            .unwrap();
            assert_eq!(committed, expected);
            assert_eq!(node.bounds(), expected.bounds);
            assert_eq!(node.size(), expected.stage.geometry.size);
        }
    }

    #[test]
    fn attachment_uses_visible_surface_edge_and_constrains_complete_shadow_box() {
        let renderer = renderer();
        let stages = vec![Stage::new(Level::Preferred, box_child(100.0, 60.0))];
        let window = Rectangle::with_size(Size::new(400.0, 300.0));
        let anchor = Rectangle::new(Point::new(150.0, 100.0), Size::new(80.0, 40.0));
        let insets = crate::popup_surface::shadow_padding();
        for side in [Side::Right, Side::Left, Side::Above, Side::Below] {
            let mut attachment =
                Attachment::new(anchor, vec![AnchorCandidate::new(side, Alignment::Start)]);
            attachment.surface_insets = insets;
            let plan = choose_attached(&stages, &renderer, window, &attachment).unwrap();
            assert!(contains(window, plan.bounds));
            match side {
                Side::Right => assert_eq!(plan.bounds.x + insets.left, anchor.x + anchor.width),
                Side::Left => {
                    assert_eq!(plan.bounds.x + plan.bounds.width - insets.right, anchor.x)
                }
                Side::Below => assert_eq!(plan.bounds.y + insets.top, anchor.y + anchor.height),
                Side::Above => {
                    assert_eq!(plan.bounds.y + plan.bounds.height - insets.bottom, anchor.y)
                }
            }
            let mut content = widget::space().into();
            let (accepted, node) = commit_attached(
                &stages,
                &renderer,
                window,
                &attachment,
                &mut content,
                &mut Tree::empty(),
            )
            .unwrap();
            assert_eq!(accepted, plan);
            assert_eq!(node.bounds(), plan.bounds);
        }
        // A visible surface fitting alone cannot certify its off-window shadow.
        let mut at_edge = Attachment::new(
            Rectangle::new(Point::new(0.0, 40.0), Size::new(20.0, 20.0)),
            vec![AnchorCandidate::new(Side::Below, Alignment::Start)],
        );
        at_edge.surface_insets = insets;
        assert!(choose_attached(&stages, &renderer, window, &at_edge).is_err());
    }

    #[test]
    fn placement_policy_orders_declared_stages_without_promoting_levels() {
        let renderer = renderer();
        let stages = vec![
            Stage::new(Level::Preferred, box_child(100.0, 80.0)),
            Stage::new(Level::Strong, box_child(100.0, 40.0)),
        ];
        let window = Rectangle::with_size(Size::new(200.0, 200.0));
        let attachment = Attachment::new(
            Rectangle::new(Point::new(0.0, 140.0), Size::new(10.0, 10.0)),
            vec![
                AnchorCandidate::new(Side::Below, Alignment::Start),
                AnchorCandidate::new(Side::Above, Alignment::Start),
            ],
        );
        let preferred = choose_attached(&stages, &renderer, window, &attachment).unwrap();
        assert_eq!(
            (preferred.stage.level, preferred.candidate_index),
            (Level::Preferred, 1)
        );
        let anchored = choose_attached(
            &stages,
            &renderer,
            window,
            &attachment.preference(PlacementPreference::AnchorFirst),
        )
        .unwrap();
        assert_eq!(
            (anchored.stage.level, anchored.candidate_index),
            (Level::Strong, 0)
        );
    }

    #[test]
    fn too_tall_native_content_is_explicit_no_fit() {
        let renderer = renderer();
        let window = Rectangle::with_size(Size::new(200.0, 100.0));
        let attachment = Attachment::below(Rectangle::new(Point::ORIGIN, Size::new(20.0, 10.0)));
        let stages = vec![Stage::new(Level::Preferred, box_child(100.0, 300.0))];
        assert_eq!(
            choose_attached(&stages, &renderer, window, &attachment),
            Err(NoFit { attempted: 4 })
        );
    }

    #[test]
    fn declared_bounded_start_fits_wide_content_without_changing_primary_side() {
        let renderer = renderer();
        let window = Rectangle::with_size(Size::new(300.0, 120.0));
        let source = Rectangle::new(Point::new(170.0, 10.0), Size::new(20.0, 20.0));
        let stages = vec![Stage::new(Level::Preferred, box_child(280.0, 30.0))];
        for alignment in [Alignment::Start, Alignment::End, Alignment::Center] {
            let strict =
                Attachment::new(source, vec![AnchorCandidate::new(Side::Below, alignment)]);
            assert!(
                choose_attached(&stages, &renderer, window, &strict).is_err(),
                "strict alignment offers retain their existing meaning"
            );
        }
        let bounded = Attachment::new(
            source,
            vec![AnchorCandidate::new(Side::Below, Alignment::StartBounded)],
        );
        let expected = choose_attached(&stages, &renderer, window, &bounded).unwrap();
        assert_eq!(
            expected.bounds,
            Rectangle::new(Point::new(20.0, 30.0), Size::new(280.0, 30.0))
        );
        assert!(contains(window, expected.bounds));
        let mut content = widget::space().into();
        let (plan, node) = commit_attached(
            &stages,
            &renderer,
            window,
            &bounded,
            &mut content,
            &mut Tree::empty(),
        )
        .unwrap();
        assert_eq!(plan, expected);
        assert_eq!(
            node.bounds(),
            expected.bounds,
            "commit uses the already-selected anchor equation"
        );
    }

    #[test]
    fn bounded_start_uses_the_offered_region_origin_on_both_axes() {
        let renderer = renderer();
        let region = Rectangle::new(Point::new(40.0, 10.0), Size::new(240.0, 100.0));
        let source = Rectangle::new(Point::new(170.0, 20.0), Size::new(20.0, 20.0));
        let stages = vec![Stage::new(Level::Preferred, box_child(220.0, 30.0))];
        let attachment = Attachment::new(
            source,
            vec![AnchorCandidate::new(Side::Below, Alignment::StartBounded)],
        );
        let plan = choose_attached(&stages, &renderer, region, &attachment).unwrap();
        assert_eq!(plan.bounds.position(), Point::new(60.0, 40.0));
        assert!(
            contains(region, plan.bounds),
            "an obstacle-free subregion is a real measurement region"
        );

        let window = Rectangle::with_size(Size::new(120.0, 300.0));
        let source = Rectangle::new(Point::new(10.0, 170.0), Size::new(20.0, 20.0));
        let stages = vec![Stage::new(Level::Preferred, box_child(30.0, 280.0))];
        let attachment = Attachment::new(
            source,
            vec![AnchorCandidate::new(Side::Right, Alignment::StartBounded)],
        );
        let plan = choose_attached(&stages, &renderer, window, &attachment).unwrap();
        assert_eq!(plan.bounds.position(), Point::new(30.0, 20.0));
        assert!(contains(window, plan.bounds));
    }

    struct Stateful;
    impl Widget<(), Theme, Renderer> for Stateful {
        fn tag(&self) -> tree::Tag {
            tree::Tag::of::<bool>()
        }
        fn state(&self) -> tree::State {
            tree::State::new(false)
        }
        fn size(&self) -> Size<Length> {
            Size::new(Length::Shrink, Length::Shrink)
        }
        fn layout(&mut self, tree: &mut Tree, _: &Renderer, _: &layout::Limits) -> layout::Node {
            layout::Node::new(Size::new(
                if *tree.state.downcast_ref::<bool>() {
                    140.0
                } else {
                    100.0
                },
                20.0,
            ))
        }
        fn draw(
            &self,
            _: &Tree,
            _: &mut Renderer,
            _: &Theme,
            _: &renderer::Style,
            _: Layout<'_>,
            _: mouse::Cursor,
            _: &Rectangle,
        ) {
        }
    }

    #[test]
    fn a_live_native_disagreement_falls_through_to_the_next_declared_stage() {
        let renderer = renderer();
        let first = child(|_| Element::new(Stateful));
        // Seed the same committed wrapper shape, then change only the native
        // retained state: fresh disposable probes still report width 100.
        let initial = first
            .query_at(Level::Preferred, &renderer, Size::new(100.0, 100.0))
            .unwrap();
        let mut content = first.render(Level::Preferred, &initial, Rc::new(Cell::new(true)));
        let mut tree = Tree::new(content.as_widget());
        content.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(100.0, 100.0)),
        );
        fn mark(tree: &mut Tree) {
            if tree.tag == tree::Tag::of::<bool>() {
                *tree.state.downcast_mut::<bool>() = true;
            }
            for child in &mut tree.children {
                mark(child);
            }
        }
        mark(&mut tree);
        let stages = vec![
            Stage::new(Level::Preferred, first),
            Stage::new(Level::Mild, box_child(60.0, 20.0)),
        ];
        let window = Rectangle::with_size(Size::new(200.0, 100.0));
        let attachment = Attachment::new(
            Rectangle::new(Point::ORIGIN, Size::new(10.0, 10.0)),
            vec![AnchorCandidate::new(Side::Below, Alignment::Start)],
        );
        let (plan, node) = commit_attached(
            &stages,
            &renderer,
            window,
            &attachment,
            &mut content,
            &mut tree,
        )
        .unwrap();
        assert_eq!(plan.stage.level, Level::Mild);
        assert_eq!(node.size(), Size::new(60.0, 20.0));
    }
}

#[cfg(test)]
mod chain_tests {
    use super::*;

    fn plan(index: usize, span: usize) -> AttachedPlan {
        AttachedPlan {
            stage: StagePlan {
                stage_index: 0,
                level: Level::Preferred,
                geometry: GeometryPlan::leaf(Size::new(20.0 * span as f32, 30.0)),
            },
            candidate_index: 0,
            bounds: Rectangle::new(
                Point::new(index as f32 * 25.0, 0.0),
                Size::new(20.0 * span as f32, 30.0),
            ),
        }
    }

    #[test]
    fn chain_scheduler_exhausts_attachment_before_declared_inline_composition() {
        let mut attempts = Vec::new();
        let selected = choose_attached_chain(
            3,
            |index| 3 - index,
            |prefix, index, span| {
                attempts.push((index, span));
                if index == 1 {
                    return Err(ChainFailure::NoFit(NoFit { attempted: 4 }));
                }
                assert!(
                    prefix
                        .last()
                        .is_none_or(|parent| parent.index + parent.span == index)
                );
                Ok(plan(index, span))
            },
        );
        assert_eq!(attempts, [(0, 1), (1, 1), (0, 2), (2, 1)]);
        assert_eq!(
            selected
                .cards
                .iter()
                .map(|card| (card.index, card.span))
                .collect::<Vec<_>>(),
            [(0, 2), (2, 1)]
        );
        assert!(selected.no_fit.is_none());
    }

    #[test]
    fn missing_sources_do_not_enable_inline_and_unusable_inline_is_no_fit() {
        let selected = choose_attached_chain(
            2,
            |_| 2,
            |_, index, span| {
                if index == 0 {
                    Ok(plan(index, span))
                } else {
                    Err(ChainFailure::MissingSource(NoFit { attempted: 0 }))
                }
            },
        );
        assert_eq!(selected.cards.len(), 1);
        assert_eq!(selected.no_fit, Some((1, NoFit { attempted: 0 })));
        let selected = choose_attached_chain(
            2,
            |_| 2,
            |_, index, span| {
                if index == 0 && span == 1 {
                    Ok(plan(index, span))
                } else {
                    Err(ChainFailure::NoFit(NoFit { attempted: 2 }))
                }
            },
        );
        assert!(selected.cards.is_empty());
        assert_eq!(selected.no_fit, Some((0, NoFit { attempted: 2 })));
    }
}
