// SPDX-License-Identifier: GPL-3.0-only

//! Informational attachments prefer obstacle-free regions before covering menus.
use super::*;
use std::cmp::Ordering;

/// Query the declared candidate order against unobstructed regions first,
/// then against the whole window. Popup rectangles are finite obstacles.
pub fn choose_hover<'a, Message: 'a>(
    stages: &[Stage<'a, Message>],
    renderer: &Renderer,
    window: Rectangle,
    attachment: &Attachment,
    obstacles: &[Rectangle],
) -> Result<AttachedPlan, NoFit> {
    let clear = regions(window, obstacles);
    for fallback in [false, true] {
        for (stage_index, candidate_index) in pairs(stages.len(), attachment) {
            let a = single_attachment(attachment, candidate_index);
            let stage = &stages[stage_index..=stage_index];
            let offers = if fallback {
                vec![window]
            } else {
                ordered_regions(stage, renderer, window, &a, &clear, obstacles)
            };
            for region in offers {
                if let Ok(mut plan) =
                    choose_attached(&stages[stage_index..=stage_index], renderer, region, &a)
                    && (fallback || !obstacles.iter().any(|r| intersects(*r, plan.bounds)))
                {
                    plan.stage.stage_index = stage_index;
                    plan.candidate_index = candidate_index;
                    return Ok(plan);
                }
            }
        }
    }
    Err(NoFit {
        attempted: stages.len() * attachment.candidates.len(),
    })
}

#[derive(Default)]
pub(crate) struct HoverRetained {
    geometry: RetainedGeometry,
    stage: Option<usize>,
}
impl HoverRetained {
    pub(crate) fn clear(&mut self) {
        self.geometry.clear();
        self.stage = None;
    }
}

// This mirrors commit_attached_retaining, with obstacles as the one extra input.
#[expect(clippy::too_many_arguments)]
pub(crate) fn commit_hover_retaining<'a, Message: 'a>(
    stages: &[Stage<'a, Message>],
    renderer: &Renderer,
    window: Rectangle,
    attachment: &Attachment,
    obstacles: &[Rectangle],
    content: &mut Element<'a, Message>,
    tree: &mut Tree,
    retained: &mut HoverRetained,
) -> Result<(AttachedPlan, layout::Node), NoFit> {
    let clear = regions(window, obstacles);
    for fallback in [false, true] {
        for (stage_index, candidate_index) in pairs(stages.len(), attachment) {
            let a = single_attachment(attachment, candidate_index);
            let stage = &stages[stage_index..=stage_index];
            let offers = if fallback {
                vec![window]
            } else {
                ordered_regions(stage, renderer, window, &a, &clear, obstacles)
            };
            for region in offers {
                // A retained one-stage plan uses a local stage index of zero;
                // invalidate it when the selected representation changes.
                if retained.stage != Some(stage_index) {
                    retained.clear();
                }
                if let Ok((mut plan, node)) = commit_attached_retaining(
                    &stages[stage_index..=stage_index],
                    renderer,
                    region,
                    &a,
                    content,
                    tree,
                    &mut retained.geometry,
                ) && (fallback || !obstacles.iter().any(|r| intersects(*r, plan.bounds)))
                {
                    retained.stage = Some(stage_index);
                    plan.stage.stage_index = stage_index;
                    plan.candidate_index = candidate_index;
                    return Ok((plan, node));
                }
            }
        }
    }
    retained.clear();
    Err(NoFit {
        attempted: stages.len() * attachment.candidates.len(),
    })
}

// Rank actual fitting geometries, rather than the size of their containing
// rectangles. This keeps a bounded alignment near its window position even
// when a larger clear strip would move it away from the source.
fn ordered_regions<'a, Message: 'a>(
    stages: &[Stage<'a, Message>],
    renderer: &Renderer,
    window: Rectangle,
    attachment: &Attachment,
    regions: &[Rectangle],
    obstacles: &[Rectangle],
) -> Vec<Rectangle> {
    let vertical = matches!(attachment.candidates[0].side, Side::Above | Side::Below);
    let cross = |bounds: Rectangle| if vertical { bounds.x } else { bounds.y };
    let reference = choose_attached(stages, renderer, window, attachment)
        .map(|plan| cross(plan.bounds))
        .unwrap_or_else(|_| cross(attachment.anchor));
    let mut offers: Vec<_> = regions
        .iter()
        .filter_map(|&region| {
            let plan = choose_attached(stages, renderer, region, attachment).ok()?;
            if obstacles
                .iter()
                .any(|obstacle| intersects(*obstacle, plan.bounds))
            {
                return None;
            }
            Some((region, (cross(plan.bounds) - reference).abs()))
        })
        .collect();
    offers.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
    offers.into_iter().map(|(region, _)| region).collect()
}

fn pairs(count: usize, attachment: &Attachment) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    match attachment.preference {
        PlacementPreference::RepresentationFirst => {
            for stage in 0..count {
                for candidate in 0..attachment.candidates.len() {
                    pairs.push((stage, candidate));
                }
            }
        }
        PlacementPreference::AnchorFirst => {
            for candidate in 0..attachment.candidates.len() {
                for stage in 0..count {
                    pairs.push((stage, candidate));
                }
            }
        }
    }
    pairs
}
fn single_attachment(attachment: &Attachment, candidate: usize) -> Attachment {
    let mut a = Attachment::new(attachment.anchor, vec![attachment.candidates[candidate]]);
    a.surface_insets = attachment.surface_insets;
    if attachment.fill_width {
        a.fill_width()
    } else {
        a
    }
}

fn intersects(a: Rectangle, b: Rectangle) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

// Empty rectangles are bounded by window or obstacle edges. Enumerating
// edge pairs also admits clear space beside a short panel, not just full strips.
fn regions(window: Rectangle, obstacles: &[Rectangle]) -> Vec<Rectangle> {
    if !window.x.is_finite()
        || !window.y.is_finite()
        || !window.width.is_finite()
        || !window.height.is_finite()
        || window.width <= 0.0
        || window.height <= 0.0
    {
        return Vec::new();
    }
    let mut xs = vec![window.x, window.x + window.width];
    let mut ys = vec![window.y, window.y + window.height];
    for o in obstacles.iter().filter_map(|o| o.intersection(&window)) {
        xs.extend([o.x, o.x + o.width]);
        ys.extend([o.y, o.y + o.height]);
    }
    for axis in [&mut xs, &mut ys] {
        axis.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
        axis.dedup();
    }
    let mut result = Vec::new();
    for (i, &x) in xs.iter().enumerate() {
        for &right in &xs[i + 1..] {
            for (j, &y) in ys.iter().enumerate() {
                for &bottom in &ys[j + 1..] {
                    let r = Rectangle {
                        x,
                        y,
                        width: right - x,
                        height: bottom - y,
                    };
                    if !obstacles.iter().any(|o| intersects(*o, r)) {
                        result.push(r);
                    }
                }
            }
        }
    }
    result.sort_by(|a, b| {
        (b.width * b.height)
            .partial_cmp(&(a.width * a.height))
            .unwrap_or(Ordering::Equal)
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic::iced::advanced::renderer::Headless;
    use cosmic::iced::{Font, Pixels, Point};

    fn renderer() -> Renderer {
        cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
            Font::default(),
            Pixels(14.0),
            Some("tiny-skia"),
        ))
        .expect("headless renderer")
    }
    fn r(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
        Rectangle {
            x,
            y,
            width,
            height,
        }
    }
    fn attachment(anchor: Rectangle) -> Attachment {
        Attachment::new(
            anchor,
            vec![
                AnchorCandidate::new(Side::Right, Alignment::Start),
                AnchorCandidate::new(Side::Left, Alignment::Start),
                AnchorCandidate::new(Side::Above, Alignment::End),
            ],
        )
    }
    #[test]
    fn finite_obstacles_leave_space_beyond_their_short_span() {
        let renderer = renderer();
        let stages: Vec<Stage<'_, ()>> = vec![Stage::new(
            Level::Preferred,
            child(|_| widget::space().width(60).height(30).into()),
        )];
        let window = r(0.0, 0.0, 300.0, 200.0);
        let source = r(100.0, 120.0, 20.0, 20.0);
        let obstacle = r(120.0, 0.0, 100.0, 100.0);
        let plan =
            choose_hover(&stages, &renderer, window, &attachment(source), &[obstacle]).unwrap();
        assert_eq!(plan.candidate_index, 0);
        assert!(!intersects(plan.bounds, obstacle));
        assert_eq!(plan.bounds.position(), Point::new(120.0, 120.0));
    }
    #[test]
    fn native_wrapped_card_falls_back_over_menu_when_clear_width_is_below_floor() {
        let renderer = renderer();
        let text =
            "A useful explanation wraps into several lines when the available width is limited.";
        let stages: Vec<Stage<'_, ()>> = vec![Stage::new(
            Level::Preferred,
            child(move |_| {
                widget::container(widget::text(text))
                    .width(Length::Shrink)
                    .into()
            })
            .minimum_width(|_, _| 100.0),
        )];
        let window = r(0.0, 0.0, 240.0, 180.0);
        let source = r(10.0, 10.0, 20.0, 20.0);
        let obstacle = r(90.0, 0.0, 150.0, 180.0);
        let a = Attachment::new(
            source,
            vec![AnchorCandidate::new(Side::Right, Alignment::Start)],
        );
        let plan = choose_hover(&stages, &renderer, window, &a, &[obstacle]).unwrap();
        assert!(intersects(plan.bounds, obstacle));
        assert!(plan.bounds.x >= 0.0 && plan.bounds.y >= 0.0);
        assert!(
            plan.bounds.x + plan.bounds.width <= window.width
                && plan.bounds.y + plan.bounds.height <= window.height
        );
        let mut body = widget::space().into();
        let (_, node) = commit_hover_retaining(
            &stages,
            &renderer,
            window,
            &a,
            &[obstacle],
            &mut body,
            &mut Tree::empty(),
            &mut HoverRetained::default(),
        )
        .unwrap();
        assert_eq!(node.bounds(), plan.bounds);
    }
    #[test]
    fn no_undeclared_flip_or_clipping() {
        let renderer = renderer();
        let stages: Vec<Stage<'_, ()>> = vec![Stage::new(
            Level::Preferred,
            child(|_| widget::space().width(100).height(50).into()),
        )];
        let a = Attachment::new(
            r(5.0, 5.0, 10.0, 10.0),
            vec![AnchorCandidate::new(Side::Above, Alignment::Start)],
        );
        assert!(choose_hover(&stages, &renderer, r(0.0, 0.0, 300.0, 200.0), &a, &[]).is_err());
    }
    #[test]
    fn declared_policy_orders_stages_and_candidates_within_clear_pass() {
        let renderer = renderer();
        let stages: Vec<Stage<'_, ()>> = vec![
            Stage::new(
                Level::Preferred,
                child(|_| widget::space().width(100).height(80).into()),
            ),
            Stage::new(
                Level::Strong,
                child(|_| widget::space().width(100).height(40).into()),
            ),
        ];
        let window = r(0.0, 0.0, 200.0, 200.0);
        let a = Attachment::new(
            r(0.0, 140.0, 10.0, 10.0),
            vec![
                AnchorCandidate::new(Side::Below, Alignment::Start),
                AnchorCandidate::new(Side::Above, Alignment::Start),
            ],
        );
        let preferred = choose_hover(&stages, &renderer, window, &a, &[]).unwrap();
        assert_eq!(
            (preferred.stage.stage_index, preferred.candidate_index),
            (0, 1)
        );
        let anchored = choose_hover(
            &stages,
            &renderer,
            window,
            &a.preference(PlacementPreference::AnchorFirst),
            &[],
        )
        .unwrap();
        assert_eq!(
            (anchored.stage.stage_index, anchored.candidate_index),
            (1, 0)
        );
    }
    #[test]
    fn bounded_cross_origin_avoids_a_finite_obstacle_without_changing_side() {
        let renderer = renderer();
        let window = r(0.0, 0.0, 300.0, 200.0);
        let obstacle = r(80.0, 0.0, 140.0, 80.0);
        let a = Attachment::new(
            r(80.0, 100.0, 20.0, 20.0),
            vec![AnchorCandidate::new(Side::Above, Alignment::StartBounded)],
        );
        let clear: Vec<Stage<'_, ()>> = vec![Stage::new(
            Level::Preferred,
            child(|_| widget::space().width(60).height(30).into()),
        )];
        let plan = choose_hover(&clear, &renderer, window, &a, &[obstacle]).unwrap();
        assert!(!intersects(plan.bounds, obstacle));
        assert_eq!(plan.bounds.y, 70.0);
        let wide: Vec<Stage<'_, ()>> = vec![Stage::new(
            Level::Preferred,
            child(|_| widget::space().width(100).height(30).into()).minimum_width(|_, _| 100.0),
        )];
        let fallback = choose_hover(&wide, &renderer, window, &a, &[obstacle]).unwrap();
        assert!(intersects(fallback.bounds, obstacle));
        assert_eq!(fallback.bounds.y, 70.0);
    }
    #[test]
    fn left_corridor_keeps_hover_aligned_with_menu_row() {
        let renderer = renderer();
        let stages: Vec<Stage<'_, ()>> = vec![Stage::new(
            Level::Preferred,
            child(|_| {
                widget::container(widget::text("Disabled action explanation"))
                    .width(168)
                    .height(40)
                    .into()
            })
            .minimum_width(|_, _| 168.0),
        )];
        let window = r(0.0, 0.0, 520.0, 700.0);
        let menu = r(272.0, 415.0, 240.0, 94.0);
        let source = r(276.0, 447.0, 232.0, 28.0);
        let attachment = Attachment::new(
            source,
            [Side::Right, Side::Left, Side::Above, Side::Below]
                .into_iter()
                .map(|side| AnchorCandidate::new(side, Alignment::StartBounded).gap(4.0))
                .collect(),
        );
        let selected = choose_hover(&stages, &renderer, window, &attachment, &[menu]).unwrap();
        assert_eq!(selected.candidate_index, 1);
        assert_eq!(selected.bounds, r(104.0, 447.0, 168.0, 40.0));
        let mut body = widget::space().into();
        let (committed, node) = commit_hover_retaining(
            &stages,
            &renderer,
            window,
            &attachment,
            &[menu],
            &mut body,
            &mut Tree::empty(),
            &mut HoverRetained::default(),
        )
        .unwrap();
        assert_eq!(committed, selected);
        assert_eq!(node.bounds(), selected.bounds);
    }
}
