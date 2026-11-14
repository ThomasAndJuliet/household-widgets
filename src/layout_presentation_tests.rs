// SPDX-License-Identifier: GPL-3.0-only

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

fn all_presentations() -> PresentationFamily {
    PresentationFamily::new(
        Presentation::default(),
        vec![
            Presentation::default(),
            Presentation {
                emphasis: Emphasis::Emphasised,
                detail: Detail::Concise,
            },
            Presentation {
                emphasis: Emphasis::Normal,
                detail: Detail::Verbose,
            },
            Presentation {
                emphasis: Emphasis::Emphasised,
                detail: Detail::Verbose,
            },
        ],
    )
}

fn fixed(width: f32, height: f32) -> Child<'static, ()> {
    child(move |_| widget::space().width(width).height(height).into())
}

fn collect(
    root: &mut ResponsiveLevels<'_, ()>,
    tree: &mut Tree,
    renderer: &Renderer,
    node: &layout::Node,
) -> Vec<AcceptedSourceReport> {
    let mut reports = SourceReports::default();
    root.operate(tree, Layout::new(node), renderer, &mut reports);
    reports.into_reports()
}

#[test]
fn presentation_axes_resolve_instance_then_immediate_then_family() {
    let family = all_presentations();
    let contextual = PresentationRequest {
        emphasis: Some(Emphasis::Emphasised),
        detail: None,
    };
    let instance = PresentationRequest {
        emphasis: Some(Emphasis::Normal),
        detail: Some(Detail::Verbose),
    };
    assert_eq!(
        family.resolve(instance, contextual).unwrap(),
        Presentation {
            emphasis: Emphasis::Normal,
            detail: Detail::Verbose,
        }
    );
    assert_eq!(
        family
            .resolve(PresentationRequest::default(), contextual)
            .unwrap(),
        Presentation {
            emphasis: Emphasis::Emphasised,
            detail: Detail::Concise,
        }
    );
    assert_eq!(
        family
            .resolve(
                PresentationRequest::default(),
                PresentationRequest::default()
            )
            .unwrap(),
        Presentation::default()
    );
    // A family can use a different ordinary presentation on either axis.
    let verbose = Presentation {
        emphasis: Emphasis::Normal,
        detail: Detail::Verbose,
    };
    let family = PresentationFamily::new(verbose, vec![verbose, Presentation::default()]);
    assert_eq!(
        family
            .resolve(
                PresentationRequest {
                    detail: Some(Detail::Concise),
                    ..Default::default()
                },
                Default::default()
            )
            .unwrap(),
        Presentation::default()
    );
}

#[test]
fn unsupported_is_explicit_skips_build_and_is_distinct_from_supported_no_fit() {
    let family = PresentationFamily::new(Presentation::default(), vec![Presentation::default()]);
    let requested = Presentation {
        emphasis: Emphasis::Emphasised,
        detail: Detail::Verbose,
    };
    let result = family.prepare(requested.into(), Default::default(), |_| -> Child<'_, ()> {
        panic!("an unsupported presentation must not invoke the factory")
    });
    assert!(matches!(result, Err(UnsupportedPresentation { requested: p }) if p == requested));
    // The caller declares its alternative; the family does not silently fall back.
    let supported = family
        .prepare(Default::default(), Default::default(), |_| {
            fixed(80.0, 20.0)
        })
        .unwrap();
    let renderer = renderer();
    assert!(matches!(
        supported.query_at(Level::Preferred, &renderer, Size::new(40.0, 40.0)),
        Err(AllocationError::NoFit { .. })
    ));
    assert!(
        choose_stage(
            &[Stage::new(Level::Preferred, supported)],
            &renderer,
            Size::new(40.0, 40.0)
        )
        .is_err()
    );
}

#[test]
fn resolved_snapshot_is_built_once_and_query_render_use_both_axes() {
    let renderer = renderer();
    for emphasis in [Emphasis::Normal, Emphasis::Emphasised] {
        for detail in [Detail::Concise, Detail::Verbose] {
            let builds = Cell::new(0);
            let requested = Presentation { emphasis, detail };
            let component = all_presentations()
                .prepare(requested.into(), Default::default(), |resolved| {
                    builds.set(builds.get() + 1);
                    fixed(
                        if resolved.emphasis == Emphasis::Emphasised {
                            70.0
                        } else {
                            50.0
                        },
                        if resolved.detail == Detail::Verbose {
                            40.0
                        } else {
                            20.0
                        },
                    )
                })
                .unwrap()
                .report_information(SourceId(1), 5, |_, _| Vec::new());
            assert_eq!(component.presentation(), Some(requested));
            let expected = component
                .query_at(Level::Mild, &renderer, Size::new(100.0, 100.0))
                .unwrap();
            let mut root = stages(vec![Stage::new(Level::Mild, component)]);
            let mut tree = Tree::empty();
            let node = root.layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, Size::new(100.0, 100.0)),
            );
            assert_eq!(
                root.selection().unwrap().as_ref().unwrap().geometry,
                expected
            );
            let reports = collect(&mut root, &mut tree, &renderer, &node);
            assert_eq!(reports.len(), 1);
            assert_eq!(reports[0].report.presentation, Some(requested));
            assert_eq!(reports[0].bounds.size(), expected.size);
            assert_eq!(builds.get(), 1);
        }
    }
}

#[test]
fn emphasised_composite_does_not_cascade_and_immediate_defaults_stay_local() {
    let family = all_presentations();
    let contextual = PresentationRequest {
        emphasis: Some(Emphasis::Emphasised),
        detail: Some(Detail::Verbose),
    };
    let explicit_normal = family
        .prepare(
            PresentationRequest {
                emphasis: Some(Emphasis::Normal),
                detail: None,
            },
            contextual,
            |_| fixed(30.0, 20.0),
        )
        .unwrap()
        .report_information(SourceId(1), 1, |_, _| Vec::new());
    let ordinary = family
        .prepare(Default::default(), Default::default(), |_| {
            fixed(30.0, 20.0)
        })
        .unwrap()
        .report_information(SourceId(2), 1, |_, _| Vec::new());
    let parent = family
        .prepare(contextual, Default::default(), |_| {
            row_child(vec![explicit_normal, ordinary], 4.0)
        })
        .unwrap()
        .report_information(SourceId(3), 1, |_, _| Vec::new());
    let renderer = renderer();
    let geometry = parent
        .query_at(Level::Preferred, &renderer, Size::new(100.0, 50.0))
        .unwrap();
    let reports = geometry.reports();
    let presentation = |id| {
        reports
            .iter()
            .find(|r| r.source == SourceId(id))
            .unwrap()
            .presentation
            .unwrap()
    };
    assert_eq!(
        presentation(1),
        Presentation {
            emphasis: Emphasis::Normal,
            detail: Detail::Verbose
        }
    );
    assert_eq!(presentation(2), Presentation::default());
    assert_eq!(
        presentation(3),
        Presentation {
            emphasis: Emphasis::Emphasised,
            detail: Detail::Verbose
        }
    );
}

fn source(revision: u64) -> Child<'static, ()> {
    child(|level| {
        widget::space()
            .width(if level == Level::Preferred {
                100.0
            } else {
                40.0
            })
            .height(20.0)
            .into()
    })
    .report_information(SourceId(10), revision, |level, _| {
        if level == Level::Preferred {
            Vec::new()
        } else {
            vec![
                InformationLoss {
                    identity: InformationId("title"),
                    kind: LossKind::Shortened,
                },
                InformationLoss {
                    identity: InformationId("name"),
                    kind: LossKind::Omitted,
                },
            ]
        }
    })
}

#[test]
fn trial_queries_do_not_publish_and_resize_refreshes_partial_loss_and_revision() {
    let renderer = renderer();
    let component = source(7);
    let alternatives = vec![
        Stage::new(Level::Preferred, component.clone()),
        Stage::new(Level::Mild, component),
    ];
    let mut root = stages(alternatives);
    let mut tree = Tree::empty();
    let wide = layout::Limits::new(Size::ZERO, Size::new(150.0, 60.0));
    let narrow = layout::Limits::new(Size::ZERO, Size::new(50.0, 60.0));
    let node = root.layout(&mut tree, &renderer, &wide);
    let before = collect(&mut root, &mut tree, &renderer, &node);
    assert!(before[0].report.losses.is_empty());
    let trial = choose_stage(&root.stages, &renderer, narrow.max()).unwrap();
    assert_eq!(trial.geometry.reports()[0].losses.len(), 2);
    assert_eq!(collect(&mut root, &mut tree, &renderer, &node), before);
    let node = root.layout(&mut tree, &renderer, &narrow);
    let compact = collect(&mut root, &mut tree, &renderer, &node);
    assert_eq!(compact[0].report.revision, 7);
    assert_eq!(compact[0].report.losses, trial.geometry.reports()[0].losses);
    assert!(
        !compact[0]
            .report
            .losses
            .iter()
            .any(|loss| loss.identity == InformationId("value"))
    );
    let node = root.layout(&mut tree, &renderer, &wide);
    assert!(
        collect(&mut root, &mut tree, &renderer, &node)[0]
            .report
            .losses
            .is_empty()
    );
    root = stages(vec![Stage::new(Level::Preferred, source(8))]);
    let node = root.layout(&mut tree, &renderer, &wide);
    let updated = collect(&mut root, &mut tree, &renderer, &node);
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].report.revision, 8);
    // A hidden or vanished source contributes no retained stale report.
    root = stages(vec![Stage::new(
        Level::Strong,
        source(8).hidden_when(|_| true),
    )]);
    let node = root.layout(&mut tree, &renderer, &wide);
    assert!(collect(&mut root, &mut tree, &renderer, &node).is_empty());
    root = stages(Vec::new());
    let node = root.layout(&mut tree, &renderer, &wide);
    assert!(collect(&mut root, &mut tree, &renderer, &node).is_empty());
}

#[test]
fn nested_forms_and_compositions_publish_each_source_once_at_accepted_bounds() {
    let leaf = fixed(30.0, 20.0).report_information(SourceId(1), 9, |_, _| Vec::new());
    let row =
        row_child(vec![fixed(10.0, 20.0), leaf], 4.0)
            .report_information(SourceId(2), 9, |_, _| Vec::new());
    let adaptive = forms(vec![
        (Level::Preferred, row),
        (Level::Strong, fixed(20.0, 20.0)),
    ])
    .report_information(SourceId(3), 9, |level, _| {
        if level >= Level::Strong {
            vec![InformationLoss {
                identity: InformationId("name"),
                kind: LossKind::Omitted,
            }]
        } else {
            Vec::new()
        }
    });
    let component = column_child(vec![fixed(40.0, 10.0), adaptive], 6.0).report_information(
        SourceId(4),
        9,
        |_, _| Vec::new(),
    );
    let renderer = renderer();
    let limits = layout::Limits::new(Size::ZERO, Size::new(100.0, 100.0));
    let mut tree = Tree::empty();
    let mut root = stages(vec![Stage::new(Level::Preferred, component.clone())]);
    let node = root.layout(&mut tree, &renderer, &limits);
    let reports = collect(&mut root, &mut tree, &renderer, &node);
    assert_eq!(reports.len(), 4);
    for id in 1..=4 {
        assert_eq!(
            reports
                .iter()
                .filter(|r| r.report.source == SourceId(id))
                .count(),
            1
        );
    }
    let leaf = reports
        .iter()
        .find(|r| r.report.source == SourceId(1))
        .unwrap();
    assert_eq!(leaf.bounds.x, 14.0);
    assert_eq!(leaf.bounds.y, 16.0);
    for report in &reports {
        assert_eq!(report.bounds.size(), report.report.size);
    }
    root = stages(vec![Stage::new(Level::Strong, component)]);
    let node = root.layout(&mut tree, &renderer, &limits);
    let reports = collect(&mut root, &mut tree, &renderer, &node);
    assert_eq!(reports.len(), 2);
    assert!(
        reports
            .iter()
            .all(|r| [SourceId(3), SourceId(4)].contains(&r.report.source))
    );
}

#[test]
fn wrapping_full_information_box_measures_at_width_without_automatic_loss() {
    let family = all_presentations();
    let component: Child<'_, ()> = family.prepare(Presentation { emphasis: Emphasis::Emphasised, detail: Detail::Verbose }.into(), Default::default(), |_| {
        child(|_| widget::text("Prepared full information wraps onto additional lines at a narrower offer.").into())
            .minimum_width(|_, _| 80.0)
    }).unwrap().report_information(SourceId(1), 1, |_, _| Vec::new());
    let renderer = renderer();
    let wide = component
        .query_at(Level::Preferred, &renderer, Size::new(320.0, 500.0))
        .unwrap();
    let narrow = component
        .query_at(Level::Preferred, &renderer, Size::new(90.0, 500.0))
        .unwrap();
    assert!(narrow.size.height > wide.size.height);
    assert!(narrow.reports()[0].losses.is_empty());
    let mut root = stages(vec![Stage::new(Level::Preferred, component)]);
    let mut tree = Tree::empty();
    root.layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(90.0, 500.0)),
    );
    assert_eq!(root.selection().unwrap().as_ref().unwrap().geometry, narrow);
}

#[test]
fn old_plan_cannot_commit_a_new_same_sized_content_or_presentation_snapshot() {
    let renderer = renderer();
    let family = all_presentations();
    let make = |presentation: Presentation, revision| {
        family
            .prepare(presentation.into(), Default::default(), |_| {
                fixed(40.0, 20.0)
            })
            .unwrap()
            .report_information(SourceId(1), revision, |_, _| Vec::new())
    };
    let stages = vec![Stage::new(
        Level::Preferred,
        make(Presentation::default(), 1),
    )];
    let window = Rectangle::with_size(Size::new(200.0, 200.0));
    let attachment = Attachment::below(Rectangle::with_size(Size::new(10.0, 10.0)));
    let plan = choose_attached(&stages, &renderer, window, &attachment).unwrap();
    for (presentation, revision) in [
        (Presentation::default(), 2),
        (
            Presentation {
                emphasis: Emphasis::Emphasised,
                detail: Detail::Concise,
            },
            1,
        ),
    ] {
        let rebuilt = vec![Stage::new(Level::Preferred, make(presentation, revision))];
        let mut content = widget::space().into();
        let mut tree = Tree::empty();
        assert!(
            plan.commit(&rebuilt, &renderer, &mut content, &mut tree)
                .is_err()
        );
        let mut reports = SourceReports::default();
        content.as_widget_mut().operate(
            &mut tree,
            Layout::new(&layout::Node::new(Size::ZERO)),
            &renderer,
            &mut reports,
        );
        assert!(reports.reports().is_empty());
    }
    // A clone is the same immutable preparation and remains a valid binding.
    let mut content = widget::space().into();
    let mut tree = Tree::empty();
    assert!(
        plan.commit(&stages, &renderer, &mut content, &mut tree)
            .is_ok()
    );
}

#[test]
fn candidate_inclusion_resolves_contextual_defaults_without_query_effects() {
    let family = all_presentations();
    let explain = |emphasis| {
        family
            .prepare(
                Default::default(),
                PresentationRequest {
                    emphasis: Some(emphasis),
                    detail: None,
                },
                |_| fixed(40.0, 20.0),
            )
            .unwrap()
            .report_information(SourceId(11), 1, |_, _| Vec::new())
    };
    let component = forms(vec![
        (
            Level::Preferred,
            column_child(vec![explain(Emphasis::Emphasised), fixed(60.0, 20.0)], 4.0),
        ),
        (Level::Strong, explain(Emphasis::Normal)),
    ]);
    let renderer = renderer();
    let mut root = stages(vec![
        Stage::new(Level::Preferred, component.clone()),
        Stage::new(Level::Strong, component),
    ]);
    let mut tree = Tree::empty();
    for (height, expected) in [(80.0, Emphasis::Emphasised), (25.0, Emphasis::Normal)] {
        let limits = layout::Limits::new(Size::ZERO, Size::new(100.0, height));
        let trial = choose_stage(&root.stages, &renderer, limits.max()).unwrap();
        let node = root.layout(&mut tree, &renderer, &limits);
        let reports = collect(&mut root, &mut tree, &renderer, &node);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].report.presentation.unwrap().emphasis, expected);
        assert_eq!(&reports[0].report, trial.geometry.reports()[0]);
    }
}

#[derive(Default)]
struct LayoutCount(u32);
struct StatefulSource;

impl Widget<(), Theme, Renderer> for StatefulSource {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<LayoutCount>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(LayoutCount::default())
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }
    fn layout(&mut self, tree: &mut Tree, _: &Renderer, _: &layout::Limits) -> layout::Node {
        let count = tree.state.downcast_mut::<LayoutCount>();
        let height = 20.0 + 10.0 * count.0 as f32;
        count.0 += 1;
        layout::Node::new(Size::new(40.0, height))
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
fn native_disagreement_publishes_only_the_next_accepted_stage() {
    let renderer = renderer();
    let changing =
        child(|_| Element::new(StatefulSource))
            .report_information(SourceId(1), 1, |_, _| Vec::new());
    let fallback = fixed(40.0, 20.0).report_information(SourceId(2), 1, |_, _| Vec::new());
    let mut root = stages(vec![
        Stage::new(Level::Preferred, changing),
        Stage::new(Level::Mild, fallback),
    ]);
    let mut tree = Tree::empty();
    let limits = layout::Limits::new(Size::ZERO, Size::new(100.0, 100.0));
    let node = root.layout(&mut tree, &renderer, &limits);
    assert_eq!(
        collect(&mut root, &mut tree, &renderer, &node)[0]
            .report
            .source,
        SourceId(1)
    );
    let node = root.layout(&mut tree, &renderer, &limits);
    assert_eq!(root.selection().unwrap().as_ref().unwrap().stage_index, 1);
    let reports = collect(&mut root, &mut tree, &renderer, &node);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].report.source, SourceId(2));
}

#[test]
fn native_frame_preserves_continuous_minima_nested_geometry_and_source_facts() {
    let content = row_child(
        vec![
            fixed(100.0, 20.0).minimum_width(|_, _| 40.0),
            fixed(20.0, 20.0).weight(0.0),
        ],
        0.0,
    )
    .report_information(SourceId(1), 4, |_, _| Vec::new());
    let framed = framed_child(content, [3.0, 5.0], |content| {
        widget::container(content).padding([3.0, 5.0]).into()
    })
    .report_information(SourceId(2), 4, |_, _| Vec::new());
    let renderer = renderer();
    assert_eq!(
        framed.query(Level::Preferred, &renderer).minimum.width,
        70.0
    );
    assert!(
        framed
            .query_at(Level::Preferred, &renderer, Size::new(69.0, 50.0))
            .is_err()
    );
    let plan = framed
        .query_at(Level::Preferred, &renderer, Size::new(90.0, 50.0))
        .unwrap();
    assert_eq!(plan.size, Size::new(90.0, 26.0));
    assert_eq!(plan.children[0].geometry.size, Size::new(80.0, 20.0));
    let mut root = stages(vec![Stage::new(Level::Preferred, framed)]);
    let mut tree = Tree::empty();
    let node = root.layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(90.0, 50.0)),
    );
    assert_eq!(root.selection().unwrap().as_ref().unwrap().geometry, plan);
    let reports = collect(&mut root, &mut tree, &renderer, &node);
    let content = reports
        .iter()
        .find(|r| r.report.source == SourceId(1))
        .unwrap();
    assert_eq!(
        content.bounds,
        Rectangle::new(cosmic::iced::Point::new(5.0, 3.0), Size::new(80.0, 20.0))
    );
    let frame = reports
        .iter()
        .find(|r| r.report.source == SourceId(2))
        .unwrap();
    assert_eq!(frame.bounds.size(), Size::new(90.0, 26.0));
}

#[test]
fn frame_rejects_undeclared_chrome_and_omits_hidden_inner_content() {
    let renderer = renderer();
    let content = fixed(40.0, 20.0).report_information(SourceId(1), 1, |_, _| Vec::new());
    let bad = framed_child(content.clone(), 2.0, |content| {
        widget::container(content).padding(8.0).into()
    });
    let mut root = stages(vec![Stage::new(Level::Preferred, bad)]);
    let mut tree = Tree::empty();
    let limits = layout::Limits::new(Size::ZERO, Size::new(100.0, 100.0));
    let node = root.layout(&mut tree, &renderer, &limits);
    assert!(root.selection().unwrap().is_err());
    assert!(collect(&mut root, &mut tree, &renderer, &node).is_empty());
    let hidden = framed_child(content.hidden_when(|_| true), 5.0, |content| {
        widget::container(content).padding(5.0).into()
    });
    let plan = hidden
        .query_at(Level::Strong, &renderer, Size::new(1.0, 1.0))
        .unwrap();
    assert!(plan.hidden);
    assert_eq!(plan.size, Size::ZERO);
    assert!(plan.reports().is_empty());
}

#[test]
fn native_region_marker_only_reports_the_accepted_stage() {
    let renderer = renderer();
    let region = |id| Region {
        id: RegionId(id),
        role: RegionRole::Section,
        revision: 7,
        section: Some(RegionId(id)),
        column: Some(RegionId(20)),
        order: Some(id as u32),
        viewport: Some(RegionId(30)),
        clips: Vec::new(),
    };
    let first = child(move |_| region_marker(Element::new(StatefulSource), region(1)));
    let second = child(move |_| region_marker(widget::space().width(40).height(20), region(2)));
    let mut root = stages(vec![
        Stage::new(Level::Preferred, first),
        Stage::new(Level::Mild, second),
    ]);
    let limits = layout::Limits::new(Size::ZERO, Size::new(100.0, 100.0));
    let mut tree = Tree::empty();
    let first_node = root.layout(&mut tree, &renderer, &limits);
    let mut first_reports = RegionReports::default();
    root.operate(
        &mut tree,
        Layout::new(&first_node),
        &renderer,
        &mut first_reports,
    );
    assert_eq!(first_reports.reports().len(), 1);
    assert_eq!(first_reports.reports()[0].region.id, RegionId(1));
    let second_node = root.layout(&mut tree, &renderer, &limits);
    assert_eq!(root.selection().unwrap().as_ref().unwrap().stage_index, 1);
    let mut reports = RegionReports::default();
    root.operate(
        &mut tree,
        Layout::new(&second_node),
        &renderer,
        &mut reports,
    );
    assert_eq!(reports.reports().len(), 1);
    assert_eq!(reports.reports()[0].region.id, RegionId(2));
    assert_eq!(reports.reports()[0].bounds.size(), Size::new(40.0, 20.0));
}

#[test]
fn native_region_reports_full_bounds_after_virtual_scroll() {
    let renderer = renderer();
    let marker = Region {
        id: RegionId(1),
        role: RegionRole::Section,
        revision: 1,
        section: None,
        column: None,
        order: None,
        viewport: None,
        clips: vec![],
    };
    let mut element: Element<'_, ()> = region_marker(widget::space().width(40).height(120), marker);
    let mut tree = Tree::new(element.as_widget());
    tree.diff(element.as_widget_mut());
    let node = element.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(100.0, 200.0)),
    );
    let mut reports = RegionReports::default();
    element.as_widget_mut().operate(
        &mut tree,
        Layout::new(&node).with_virtual_offset(Vector::new(0.0, 35.0)),
        &renderer,
        &mut reports,
    );
    assert_eq!(reports.reports()[0].bounds.y, -35.0);
    assert_eq!(reports.reports()[0].bounds.height, 120.0);
}

#[test]
fn region_observer_reports_initial_reflow_and_revision_without_redraw_loop() {
    let renderer = renderer();
    let make = |revision| {
        observe_regions(
            region_marker(
                widget::space().width(Length::Fill).height(120),
                Region {
                    id: RegionId(1),
                    role: RegionRole::Section,
                    revision,
                    section: None,
                    column: None,
                    order: None,
                    viewport: None,
                    clips: vec![],
                },
            ),
            |reports| reports,
        )
    };
    let mut view = make(1);
    let mut tree = Tree::new(view.as_widget());
    let redraw = |view: &mut Element<'_, RegionReports>, tree: &mut Tree, width, invalid| {
        let bounds = Size::new(width, 200.0);
        let node =
            view.as_widget_mut()
                .layout(tree, &renderer, &layout::Limits::new(Size::ZERO, bounds));
        let mut messages = vec![];
        let mut shell = Shell::new(&mut messages);
        if invalid {
            shell.invalidate_layout();
        }
        view.as_widget_mut().update(
            tree,
            &Event::Window(cosmic::iced::window::Event::RedrawRequested(
                std::time::Instant::now(),
            )),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut cosmic::iced::advanced::clipboard::Null,
            &mut shell,
            &Rectangle::with_size(bounds),
        );
        messages
    };
    assert!(redraw(&mut view, &mut tree, 50.0, true).is_empty());
    let first = redraw(&mut view, &mut tree, 50.0, false);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].reports()[0].bounds.width, 50.0);
    assert!(redraw(&mut view, &mut tree, 50.0, false).is_empty());
    view = make(1);
    tree.diff(view.as_widget_mut());
    assert!(redraw(&mut view, &mut tree, 50.0, false).is_empty());
    let resized = redraw(&mut view, &mut tree, 90.0, false);
    assert_eq!(resized.len(), 1);
    assert_eq!(resized[0].reports()[0].bounds.width, 90.0);
    view = make(2);
    tree.diff(view.as_widget_mut());
    let revised = redraw(&mut view, &mut tree, 90.0, false);
    assert_eq!(revised.len(), 1);
    assert_eq!(revised[0].reports()[0].region.revision, 2);
}

#[test]
fn region_and_scroll_observers_preserve_distinct_native_size_hints() {
    struct Hinted {
        content: Element<'static, ()>,
    }
    impl Widget<(), Theme, Renderer> for Hinted {
        forward_widget!((), Theme, Renderer; children, diff, size, layout, draw);
        fn size_hint(&self) -> Size<Length> {
            Size::new(Length::Fill, Length::FillPortion(2))
        }
    }
    let source = || {
        Element::new(Hinted {
            content: widget::space().width(20).height(20).into(),
        })
    };
    let expected = source().as_widget().size_hint();
    assert_ne!(source().as_widget().size(), expected);
    let marker = region_marker(
        source(),
        Region {
            id: RegionId(1),
            role: RegionRole::Section,
            revision: 1,
            section: None,
            column: None,
            order: None,
            viewport: None,
            clips: vec![],
        },
    );
    assert_eq!(marker.as_widget().size_hint(), expected);
    assert_eq!(
        observe_regions(source(), |_| ()).as_widget().size_hint(),
        expected
    );
    assert_eq!(
        crate::scroll_intent::vertical(source(), |content| content, || ())
            .as_widget()
            .size_hint(),
        expected
    );
}
