// SPDX-License-Identifier: GPL-3.0-only

// Acceptance tests use the renderer's actual measurements and public contract,
// rather than the layout engine's internal allocation helpers.
use super::*;
use cosmic::iced::advanced::renderer::Headless;
use cosmic::iced::{Font, Pixels, Point};

fn renderer() -> Renderer {
    cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
        Font::default(),
        Pixels(14.0),
        Some("tiny-skia"),
    ))
    .expect("headless tiny-skia renderer")
}

fn box_at_levels() -> Child<'static, u8> {
    child(|level| {
        let width = if level == Level::Preferred {
            80.0
        } else {
            50.0
        };
        widget::Space::new().width(width).height(20.0).into()
    })
}

fn pair_row() -> Child<'static, u8> {
    row_child(vec![box_at_levels(), box_at_levels()], 0.0)
}

fn pair_wrap() -> Child<'static, u8> {
    wrap_child(vec![box_at_levels(), box_at_levels()], 0.0)
}

#[test]
fn stage_order_controls_whether_layout_or_compaction_happens_first() {
    let renderer = renderer();
    let layout_first = vec![
        Stage::new(Level::Preferred, pair_row()),
        Stage::new(Level::Preferred, pair_wrap()),
        Stage::new(Level::Mild, pair_row()),
    ];
    let compact_first = vec![
        Stage::new(Level::Preferred, pair_row()),
        Stage::new(Level::Mild, pair_row()),
        Stage::new(Level::Preferred, pair_wrap()),
    ];
    let available = Size::new(100.0, 40.0);
    let layout = choose_stage(&layout_first, &renderer, available).unwrap();
    assert_eq!(layout.stage_index, 1);
    assert_eq!(layout.level, Level::Preferred);
    let compact = choose_stage(&compact_first, &renderer, available).unwrap();
    assert_eq!(compact.stage_index, 1);
    assert_eq!(compact.level, Level::Mild);
}

#[test]
fn width_fitting_wrap_is_rejected_when_its_total_height_does_not_fit() {
    let renderer = renderer();
    let alternatives = vec![
        Stage::new(Level::Preferred, pair_wrap()),
        Stage::new(Level::Mild, pair_row()),
    ];
    let selected = choose_stage(&alternatives, &renderer, Size::new(100.0, 25.0)).unwrap();
    assert_eq!(selected.stage_index, 1);
    assert_eq!(selected.level, Level::Mild);
    assert!(choose_stage(&alternatives[..1], &renderer, Size::new(100.0, 25.0)).is_err());
}

#[test]
fn hidden_child_costs_no_width_or_spacing_and_primary_remains() {
    let renderer = renderer();
    let fixed = |width| child(move |_| widget::Space::new().width(width).height(20.0).into());
    let optional: Child<'_, u8> = fixed(30.0).hidden_when(|level| level >= Level::Strong);
    let primary = fixed(40.0);
    let row = row_child(vec![primary, optional, fixed(20.0)], 5.0);
    let shown = row
        .query_at(Level::Preferred, &renderer, Size::new(200.0, 40.0))
        .unwrap();
    let hidden = row
        .query_at(Level::Strong, &renderer, Size::new(200.0, 40.0))
        .unwrap();
    assert_eq!(shown.size.width, 100.0);
    assert_eq!(hidden.size.width, 65.0);
    assert_eq!(hidden.size.height, 20.0);
}

#[test]
fn no_fit_is_explicit_and_does_not_dispatch_previously_visible_control() {
    let renderer = renderer();
    let mut tree = Tree::empty();
    let mut root = stages(vec![Stage::new(
        Level::Preferred,
        child(|_| {
            widget::button::text("Action")
                .width(Length::Fixed(120.0))
                .on_press(7)
                .into()
        }),
    )]);
    let wide = root.layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(200.0, 100.0)),
    );
    assert!(root.selection().unwrap().is_ok());
    assert!(wide.size().width >= 120.0);
    let mut before = Vec::new();
    for event in [
        mouse::Event::ButtonPressed(mouse::Button::Left),
        mouse::Event::ButtonReleased(mouse::Button::Left),
    ] {
        root.update(
            &mut tree,
            &Event::Mouse(event),
            Layout::new(&wide),
            mouse::Cursor::Available(Point::new(8.0, 8.0)),
            &renderer,
            &mut cosmic::iced::advanced::clipboard::Null,
            &mut Shell::new(&mut before),
            &Rectangle {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            },
        );
    }
    assert_eq!(before, vec![7]);
    let narrow = root.layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(50.0, 30.0)),
    );
    assert!(root.selection().unwrap().is_err());
    let mut messages = Vec::new();
    for event in [
        mouse::Event::ButtonPressed(mouse::Button::Left),
        mouse::Event::ButtonReleased(mouse::Button::Left),
    ] {
        root.update(
            &mut tree,
            &Event::Mouse(event),
            Layout::new(&narrow),
            mouse::Cursor::Available(Point::new(8.0, 8.0)),
            &renderer,
            &mut cosmic::iced::advanced::clipboard::Null,
            &mut Shell::new(&mut messages),
            &Rectangle {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            },
        );
    }
    assert!(messages.is_empty());
}

#[test]
fn nested_groups_receive_the_exact_level_and_wrap_as_immediate_children() {
    let renderer = renderer();
    let fixed = |width| child(move |_| widget::Space::new().width(width).height(20.0).into());
    let nested = row_child(vec![box_at_levels(), box_at_levels()], 0.0);
    let outer = wrap_child(vec![nested, fixed(40.0)], 0.0);
    // At mild, the nested row is 100 wide and must stay together: it and
    // the 40-wide sibling need two rows in 120 pixels.
    let geometry = outer
        .query_at(Level::Mild, &renderer, Size::new(120.0, 40.0))
        .unwrap();
    assert_eq!(geometry.size.height, 40.0);
    assert!(
        outer
            .query_at(Level::Preferred, &renderer, Size::new(120.0, 40.0))
            .is_err()
    );
}

#[test]
fn scroll_viewport_must_fit_its_declared_outer_minimum_not_all_content() {
    let renderer = renderer();
    let scroll: Child<'_, u8> = scroll_child(
        |_| {
            cosmic::iced::widget::scrollable(widget::Space::new().width(500.0).height(500.0)).into()
        },
        Size::new(80.0, 40.0),
    );
    assert!(
        scroll
            .query_at(Level::Preferred, &renderer, Size::new(79.0, 100.0))
            .is_err()
    );
    assert!(
        scroll
            .query_at(Level::Preferred, &renderer, Size::new(100.0, 39.0))
            .is_err()
    );
    assert!(
        scroll
            .query_at(Level::Preferred, &renderer, Size::new(80.0, 40.0))
            .is_ok()
    );
}

#[test]
fn column_remeasures_wrapping_text_at_its_allocated_width() {
    let renderer = renderer();
    let paragraph = || {
        child(|_| {
            widget::text("A long paragraph with several words that must wrap on a narrow display.")
                .into()
        })
        .minimum_width(|_, _| 40.0)
    };
    let column: Child<'_, u8> = column_child(vec![paragraph()], 0.0);
    let wide = column
        .query_at(Level::Preferred, &renderer, Size::new(400.0, 400.0))
        .unwrap();
    let narrow = column
        .query_at(Level::Preferred, &renderer, Size::new(100.0, 400.0))
        .unwrap();
    assert!(narrow.size.height > wide.size.height);
    let mut root = stages(vec![Stage::new(Level::Preferred, column)]);
    let mut tree = Tree::empty();
    let node = root.layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(100.0, 400.0)),
    );
    assert!((node.size().height - narrow.size.height).abs() < 0.5);
}

#[test]
fn each_stage_restarts_allocation_and_resize_restores_preferred_forms() {
    let renderer = renderer();
    let title = || {
        child(|_| widget::Space::new().width(100.0).height(20.0).into()).minimum_width(|_, _| 40.0)
    };
    let menu = || {
        child(|level| {
            let width = if level == Level::Preferred {
                100.0
            } else {
                40.0
            };
            widget::Space::new().width(width).height(20.0).into()
        })
    };
    let make_row = || -> Child<'static, u8> { row_child(vec![title(), menu()], 0.0) };
    let alternatives = vec![
        Stage::new(Level::Preferred, make_row()),
        Stage::new(Level::Mild, make_row()),
    ];
    let narrow = choose_stage(&alternatives, &renderer, Size::new(130.0, 40.0)).unwrap();
    assert_eq!(narrow.level, Level::Mild);
    assert!((narrow.geometry.children[0].geometry.size.width - 90.0).abs() < 0.01);
    let wide = choose_stage(&alternatives, &renderer, Size::new(200.0, 40.0)).unwrap();
    assert_eq!(wide.level, Level::Preferred);
    assert_eq!(wide.geometry.children[0].geometry.size.width, 100.0);
    assert_eq!(wide.geometry.children[1].geometry.size.width, 100.0);
}

#[test]
fn nested_scroll_viewport_respects_the_remaining_column_height() {
    let renderer = renderer();
    let header = || child(|_| widget::Space::new().width(80.0).height(20.0).into());
    let viewport = || {
        scroll_child(
            |_| {
                cosmic::iced::widget::scrollable(widget::Space::new().width(500.0).height(500.0))
                    .into()
            },
            Size::new(80.0, 40.0),
        )
    };
    let column: Child<'_, u8> = column_child(vec![header(), viewport()], 5.0);
    assert!(
        column
            .query_at(Level::Preferred, &renderer, Size::new(100.0, 64.0))
            .is_err()
    );
    let geometry = column
        .query_at(Level::Preferred, &renderer, Size::new(100.0, 75.0))
        .unwrap();
    assert!(geometry.size.height <= 75.0);
    assert!(geometry.children[1].geometry.size.height >= 40.0);
    assert!(geometry.children[1].geometry.size.height < 500.0);
    let mut root = stages(vec![Stage::new(Level::Preferred, column)]);
    let mut tree = Tree::empty();
    let node = root.layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(100.0, 75.0)),
    );
    assert!(root.selection().unwrap().is_ok());
    assert!((node.size().height - geometry.size.height).abs() < 0.5);
}

#[test]
fn entirely_hidden_nested_group_does_not_leave_an_outer_gap() {
    let renderer = renderer();
    let fixed = |width| child(move |_| widget::Space::new().width(width).height(20.0).into());
    let hidden: Child<'_, u8> = row_child(vec![fixed(30.0).hidden_when(|_| true)], 8.0);
    let row = row_child(vec![fixed(40.0), hidden, fixed(20.0)], 5.0);
    let plan = row
        .query_at(Level::Preferred, &renderer, Size::new(200.0, 40.0))
        .unwrap();
    assert_eq!(plan.size.width, 65.0);
    assert_eq!(plan.children.len(), 2);
}

#[test]
fn explicit_fallback_is_the_only_action_when_no_stage_fits() {
    let renderer = renderer();
    let mut root = stages(vec![Stage::new(
        Level::Preferred,
        child(|_| {
            widget::button::text("Original")
                .width(Length::Fixed(150.0))
                .on_press(7)
                .into()
        }),
    )])
    .fallback(|| widget::button::text("Fallback").on_press(9).into());
    let mut tree = Tree::empty();
    let node = root.layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(100.0, 80.0)),
    );
    assert!(root.selection().unwrap().is_err());
    let mut messages = Vec::new();
    for event in [
        mouse::Event::ButtonPressed(mouse::Button::Left),
        mouse::Event::ButtonReleased(mouse::Button::Left),
    ] {
        root.update(
            &mut tree,
            &Event::Mouse(event),
            Layout::new(&node),
            mouse::Cursor::Available(Point::new(8.0, 8.0)),
            &renderer,
            &mut cosmic::iced::advanced::clipboard::Null,
            &mut Shell::new(&mut messages),
            &Rectangle {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 80.0,
            },
        );
    }
    assert_eq!(messages, vec![9]);
}

#[test]
fn hidden_native_controls_have_no_event_or_overlay_target() {
    let renderer = renderer();
    let visible = child(|_| {
        widget::button::text("Visible")
            .width(Length::Fixed(80.0))
            .on_press(7)
            .into()
    });
    let hidden =
        child(|_| widget::dropdown(&["First", "Second"], Some(0), |i| i as u8 + 20).into())
            .hidden_when(|_| true);
    let mut root = stages(vec![Stage::new(
        Level::Preferred,
        row_child(vec![visible, hidden], 0.0),
    )]);
    let mut tree = Tree::empty();
    let node = root.layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(200.0, 80.0)),
    );
    let viewport = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 80.0,
    };
    let mut messages = Vec::new();
    for (x, expected) in [(8.0, vec![7]), (100.0, vec![])] {
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            root.update(
                &mut tree,
                &Event::Mouse(event),
                Layout::new(&node),
                mouse::Cursor::Available(Point::new(x, 8.0)),
                &renderer,
                &mut cosmic::iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &viewport,
            );
        }
        assert_eq!(messages, expected);
        messages.clear();
    }
    assert!(
        root.overlay(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &viewport,
            Vector::ZERO
        )
        .is_none()
    );
    #[cfg(feature = "a11y")]
    {
        let actual = root.a11y_nodes(Layout::new(&node), &tree, mouse::Cursor::Unavailable);
        let mut baseline = stages(vec![Stage::new(
            Level::Preferred,
            row_child(
                vec![child(|_| {
                    widget::button::text("Visible")
                        .width(Length::Fixed(80.0))
                        .on_press(7)
                        .into()
                })],
                0.0,
            ),
        )]);
        let mut baseline_tree = Tree::empty();
        let baseline_node = baseline.layout(
            &mut baseline_tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(200.0, 80.0)),
        );
        let expected = baseline.a11y_nodes(
            Layout::new(&baseline_node),
            &baseline_tree,
            mouse::Cursor::Unavailable,
        );
        assert_eq!(actual.root().len(), expected.root().len());
        assert_eq!(actual.children().len(), expected.children().len());
        assert!(!actual.root().is_empty());
    }
}

#[test]
fn rejected_named_stage_probe_does_not_steal_focused_state() {
    use cosmic::iced::advanced::widget::operation::Focusable;
    use cosmic::iced::keyboard;
    struct Focus;
    impl Operation for Focus {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn focusable(
            &mut self,
            _: Option<&cosmic::iced::advanced::widget::Id>,
            _: Rectangle,
            state: &mut dyn Focusable,
        ) {
            state.focus();
        }
    }
    fn named(width: f32) -> Child<'static, u8> {
        child(move |_| {
            widget::responsive(move |_| {
                widget::button::text("Retained")
                    .id(cosmic::iced::advanced::widget::Id::new("contract-focus"))
                    .width(Length::Fixed(width))
                    .on_press(7)
                    .into()
            })
            .width(Length::Fixed(width))
            .height(Length::Shrink)
            .into()
        })
    }
    fn make() -> ResponsiveLevels<'static, u8> {
        stages(vec![
            Stage::new(Level::Preferred, named(200.0)),
            Stage::new(Level::Mild, named(100.0)),
        ])
    }
    fn rebuild(
        tree: &mut Tree,
        renderer: &Renderer,
    ) -> (ResponsiveLevels<'static, u8>, layout::Node) {
        let mut root = make();
        tree::NAMED.with(|named| *named.borrow_mut() = tree.take_all_named());
        tree.diff(&mut root as &mut dyn Widget<u8, Theme, Renderer>);
        let node = root.layout(
            tree,
            renderer,
            &layout::Limits::new(Size::ZERO, Size::new(140.0, 80.0)),
        );
        tree::NAMED.with(|named| named.borrow_mut().clear());
        (root, node)
    }
    let renderer = renderer();
    let mut tree = Tree::empty();
    let (mut root, node) = rebuild(&mut tree, &renderer);
    assert_eq!(root.selection().unwrap().as_ref().unwrap().stage_index, 1);
    root.operate(&mut tree, Layout::new(&node), &renderer, &mut Focus);
    let (mut root, node) = rebuild(&mut tree, &renderer);
    assert_eq!(root.selection().unwrap().as_ref().unwrap().stage_index, 1);
    let event = Event::Keyboard(keyboard::Event::KeyPressed {
        key: keyboard::Key::Named(keyboard::key::Named::Enter),
        modified_key: keyboard::Key::Named(keyboard::key::Named::Enter),
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Enter),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::empty(),
        text: None,
        repeat: false,
    });
    let mut messages = Vec::new();
    root.update(
        &mut tree,
        &event,
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut Shell::new(&mut messages),
        &Rectangle {
            x: 0.0,
            y: 0.0,
            width: 140.0,
            height: 80.0,
        },
    );
    assert_eq!(messages, vec![7]);
}
