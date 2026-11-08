// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use std::cell::Cell;
use std::rc::Rc;

use cosmic::iced::advanced::renderer::Headless;
use cosmic::iced::advanced::widget::operation::Focusable;
use cosmic::iced::{Font, Pixels, Point, keyboard};

const VIEWPORT: Rectangle = Rectangle {
    x: 0.0,
    y: 0.0,
    width: 800.0,
    height: 400.0,
};

#[test]
fn first_fit_respects_preference_bounds_height_and_explicit_failure() {
    let measured = |level| match level {
        Level::Preferred | Level::Mild => Size::new(160.0, 48.0),
        Level::Strong => Size::new(100.0, 40.0),
        Level::AllPermitted => Size::new(60.0, 40.0),
    };
    assert_eq!(
        first_fitting(Level::AllPermitted, Size::new(160.0, 48.0), measured),
        Some(Level::Preferred)
    );
    assert_eq!(
        first_fitting(Level::AllPermitted, Size::new(160.0, 40.0), measured),
        Some(Level::Strong)
    );
    assert_eq!(
        first_fitting(Level::Mild, Size::new(100.0, 48.0), measured),
        None
    );
    assert_eq!(
        first_fitting(Level::AllPermitted, Size::new(59.0, 48.0), measured),
        None
    );
    assert_eq!(
        first_fitting(Level::AllPermitted, Size::INFINITE, |_| Size::new(
            f32::NAN,
            0.0
        )),
        None
    );
}

fn capacity(preferred: f32, minimum: f32, weight: f32) -> WidthCapacity {
    WidthCapacity {
        preferred,
        minimum,
        weight,
    }
}

#[test]
fn three_nested_rows_query_and_commit_one_parent_selected_level() {
    let renderer = renderer();
    let committed = Rc::new(Cell::new(Level::AllPermitted));
    let observed = committed.clone();
    let flexible: Child<'_, u8> = child(move |level| {
        observed.set(level);
        widget::Space::new().width(160.0).height(20.0).into()
    })
    .minimum_width(|level, preferred| {
        if level >= Level::Mild {
            60.0
        } else {
            preferred.width
        }
    });
    let fixed = || child(|_| widget::Space::new().width(10.0).height(20.0).into()).weight(0.0);
    let inner = row_child(vec![flexible, fixed()], 0.0);
    let middle = row_child(vec![inner, fixed()], 0.0);
    let outer = row_child(vec![middle, fixed()], 0.0);
    assert_eq!(
        outer.query(Level::Preferred, &renderer).minimum.width,
        190.0
    );
    assert_eq!(outer.query(Level::Mild, &renderer).minimum.width, 90.0);
    let mut widget = responsive_child(Level::AllPermitted, outer);
    let mut tree = Tree::empty();
    let node = widget.layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(100.0, 40.0)),
    );
    assert_eq!(committed.get(), Level::Mild);
    assert_eq!(node.size().width, 100.0);
    let _ = widget.layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(220.0, 40.0)),
    );
    assert_eq!(committed.get(), Level::Preferred);
}

#[test]
fn capacity_sharing_uses_original_rates_clamps_and_protects_zero_weight() {
    let children = [
        capacity(100.0, 80.0, 3.0),
        capacity(100.0, 0.0, 1.0),
        capacity(40.0, 0.0, 0.0),
    ];
    // Original rates are 60 and 100. At 160 the first child reaches its
    // minimum, leaving the remaining shortage entirely to the second.
    assert_eq!(allocate_width(160.0, &children), Ok(vec![80.0, 40.0, 40.0]));
    assert_eq!(
        allocate_width(300.0, &children),
        Ok(vec![100.0, 100.0, 40.0])
    );
    assert_eq!(
        allocate_width(119.0, &children),
        Err(AllocationError::NoFit {
            required: 120.0,
            available: 119.0
        })
    );
    let weighted = allocate_width(
        150.0,
        &[capacity(100.0, 0.0, 2.0), capacity(100.0, 0.0, 1.0)],
    )
    .unwrap();
    assert!((weighted.iter().sum::<f32>() - 150.0).abs() < 0.001);
    assert!(((100.0 - weighted[0]) / (100.0 - weighted[1]) - 2.0).abs() < 0.001);
    assert_eq!(allocate_width(0.0, &[]), Ok(vec![]));
    assert_eq!(
        allocate_width(10.0, &[capacity(1.0, 2.0, 1.0)]),
        Err(AllocationError::InvalidInput)
    );
}

fn renderer() -> Renderer {
    cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
        Font::default(),
        Pixels(14.0),
        Some("tiny-skia"),
    ))
    .expect("headless tiny-skia renderer")
}

// Rebuild exactly at the native seam: extract named states before diff/layout,
// and clear unused named states afterwards, as UserInterface::build does.
fn rebuild(
    tree: &mut Tree,
    renderer: &Renderer,
    width: f32,
    view: impl Fn(Level) -> Element<'static, u8> + 'static,
) -> (ResponsiveLevels<'static, u8>, layout::Node) {
    reconcile(tree, renderer, width, responsive(Level::AllPermitted, view))
}

fn reconcile(
    tree: &mut Tree,
    renderer: &Renderer,
    width: f32,
    mut widget: ResponsiveLevels<'static, u8>,
) -> (ResponsiveLevels<'static, u8>, layout::Node) {
    tree::NAMED.with(|named| *named.borrow_mut() = tree.take_all_named());
    tree.diff(&mut widget as &mut dyn Widget<u8, Theme, Renderer>);
    let node = widget.layout(
        tree,
        renderer,
        &layout::Limits::new(Size::ZERO, Size::new(width, VIEWPORT.height)),
    );
    tree::NAMED.with(|named| named.borrow_mut().clear());
    (widget, node)
}

fn event(
    widget: &mut ResponsiveLevels<'_, u8>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &Renderer,
    event: Event,
) -> Vec<u8> {
    let mut messages = Vec::new();
    widget.update(
        tree,
        &event,
        Layout::new(node),
        mouse::Cursor::Available(Point::new(8.0, 8.0)),
        renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut Shell::new(&mut messages),
        &VIEWPORT,
    );
    messages
}

struct Focus;
impl Operation for Focus {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn focusable(
        &mut self,
        _id: Option<&cosmic::iced::advanced::widget::Id>,
        _bounds: Rectangle,
        state: &mut dyn Focusable,
    ) {
        state.focus();
    }
}

fn enter() -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: keyboard::Key::Named(keyboard::key::Named::Enter),
        modified_key: keyboard::Key::Named(keyboard::key::Named::Enter),
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Enter),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::empty(),
        text: None,
        repeat: false,
    })
}

fn named_button(level: Level) -> Element<'static, u8> {
    let width = match level {
        Level::Preferred => 160.0,
        Level::Mild => 120.0,
        Level::Strong => 80.0,
        Level::AllPermitted => 48.0,
    };
    // Nested Responsive performs its own diff during layout. The discarded
    // probes must not consume the committed button's named focus state there.
    widget::responsive(move |_| {
        widget::button::text("Section")
            .id(cosmic::iced::advanced::widget::Id::new("retained-tab"))
            .width(Length::Fixed(width))
            .on_press(7)
            .into()
    })
    .width(Length::Fixed(width))
    .height(Length::Shrink)
    .into()
}

#[test]
fn native_focus_survives_same_form_level_change_and_discarded_named_probes() {
    let renderer = renderer();
    let mut tree = Tree::empty();
    let (mut widget, node) = rebuild(&mut tree, &renderer, 200.0, named_button);
    widget.operate(&mut tree, Layout::new(&node), &renderer, &mut Focus);
    assert_eq!(
        event(&mut widget, &mut tree, &node, &renderer, enter()),
        vec![7]
    );
    let (mut widget, node) = rebuild(&mut tree, &renderer, 140.0, named_button);
    assert_eq!(
        event(&mut widget, &mut tree, &node, &renderer, enter()),
        vec![7]
    );
    let (mut widget, node) = rebuild(&mut tree, &renderer, 200.0, named_button);
    assert_eq!(
        event(&mut widget, &mut tree, &node, &renderer, enter()),
        vec![7]
    );
}

#[test]
fn real_text_measurement_controls_exact_fit_and_growth_without_thresholds() {
    let renderer = renderer();
    let label = "Overview of all household machines";
    let mut text: Element<'static, u8> = widget::button::text(label).on_press(0).into();
    let mut text_tree = Tree::new(text.as_widget());
    text_tree.diff(text.as_widget_mut());
    let required = text
        .as_widget_mut()
        .layout(
            &mut text_tree,
            &renderer,
            &layout::Limits::NONE
                .width(Length::Shrink)
                .height(Length::Shrink),
        )
        .size()
        .width;
    let selected = Rc::new(Cell::new(Level::AllPermitted));
    let build = |selected: Rc<Cell<Level>>| {
        move |level| {
            selected.set(level);
            widget::button::text(if level <= Level::Mild { label } else { "All" })
                .on_press(0)
                .into()
        }
    };
    let mut tree = Tree::empty();
    let _ = rebuild(&mut tree, &renderer, required, build(selected.clone()));
    assert_eq!(selected.get(), Level::Preferred);
    let _ = rebuild(
        &mut tree,
        &renderer,
        required - 1.0,
        build(selected.clone()),
    );
    assert_eq!(selected.get(), Level::Strong);
    let _ = rebuild(
        &mut tree,
        &renderer,
        required + 1.0,
        build(selected.clone()),
    );
    assert_eq!(selected.get(), Level::Preferred);
    let (widget, node) = rebuild(&mut tree, &renderer, 0.0, build(selected.clone()));
    assert!(widget.selection().expect("layout was attempted").is_err());
    assert!(node.size().width.is_finite() && node.size().height.is_finite());
}

fn dropdown_or_row(level: Level) -> Element<'static, u8> {
    const NAMES: &[&str] = &["Overview", "Machines"];
    if level == Level::AllPermitted {
        widget::dropdown(NAMES, Some(0), |index| index as u8).into()
    } else {
        widget::row::with_capacity(1)
            .push(
                widget::button::text("Overview")
                    .width(Length::Fixed(400.0))
                    .on_press(7),
            )
            .into()
    }
}

fn has_overlay(
    widget: &mut ResponsiveLevels<'_, u8>,
    tree: &mut Tree,
    node: &layout::Node,
    renderer: &Renderer,
) -> bool {
    widget
        .overlay(tree, Layout::new(node), renderer, &VIEWPORT, Vector::ZERO)
        .is_some()
}

#[test]
fn native_dropdown_stays_open_on_same_form_resize_and_drops_with_replacement() {
    let renderer = renderer();
    let mut tree = Tree::empty();
    let (mut widget, node) = rebuild(&mut tree, &renderer, 200.0, dropdown_or_row);
    assert!(!has_overlay(&mut widget, &mut tree, &node, &renderer));
    assert!(
        event(
            &mut widget,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        )
        .is_empty()
    );
    assert!(has_overlay(&mut widget, &mut tree, &node, &renderer));
    let (mut widget, node) = rebuild(&mut tree, &renderer, 180.0, dropdown_or_row);
    assert!(has_overlay(&mut widget, &mut tree, &node, &renderer));
    let (mut widget, node) = rebuild(&mut tree, &renderer, 800.0, dropdown_or_row);
    assert!(!has_overlay(&mut widget, &mut tree, &node, &renderer));
    assert!(
        event(
            &mut widget,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        )
        .is_empty()
    );
    assert_eq!(
        event(
            &mut widget,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
        ),
        vec![7]
    );
}

#[test]
fn recursive_row_keeps_native_dropdown_open_on_rebuild_at_the_same_constraint() {
    let renderer = renderer();
    let mut tree = Tree::empty();
    let root = || {
        responsive_child(
            Level::AllPermitted,
            row_child(vec![child(dropdown_or_row)], 0.0),
        )
    };
    let (mut widget, node) = reconcile(&mut tree, &renderer, 200.0, root());
    assert!(
        event(
            &mut widget,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        )
        .is_empty()
    );
    assert!(has_overlay(&mut widget, &mut tree, &node, &renderer));
    let (mut widget, node) = reconcile(&mut tree, &renderer, 200.0, root());
    assert!(has_overlay(&mut widget, &mut tree, &node, &renderer));
}

#[cfg(feature = "a11y")]
#[test]
fn accessibility_forwards_only_the_selected_native_button() {
    let renderer = renderer();
    let mut tree = Tree::empty();
    let (widget, node) = rebuild(&mut tree, &renderer, 140.0, |level| {
        widget::button::text(format!("{level:?}"))
            .on_press(0)
            .into()
    });
    let nodes = widget.a11y_nodes(Layout::new(&node), &tree, mouse::Cursor::Unavailable);
    assert_eq!(nodes.root().len(), 1);
}

#[test]
fn elastic_gap_is_measured_at_its_minimum_and_keeps_actions_at_the_right() {
    let renderer = renderer();
    let fixed = |width| child(move |_| widget::Space::new().width(width).height(32).into());
    let component: Child<'_, u8> = row_child(vec![fixed(40.0), spacer(8.0), fixed(32.0)], 0.0);
    assert_eq!(
        component.query(Level::Preferred, &renderer).minimum.width,
        80.0
    );
    let mut root = responsive_child(Level::AllPermitted, component);
    let mut tree = Tree::empty();
    for width in [200.0, 80.0] {
        let node = root.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(width, 32.0)),
        );
        let row = &node.children()[0];
        assert_eq!(row.children()[0].size().width, 40.0);
        assert_eq!(row.children()[2].size().width, 32.0);
        assert_eq!(row.children()[2].bounds().x, width - 32.0);
        assert_eq!(row.children()[1].size().width, width - 72.0);
    }
}

#[test]
fn surplus_shares_follow_grow_weights_without_changing_intrinsic_queries() {
    let renderer = renderer();
    let component: Child<'_, u8> =
        row_child(vec![spacer(4.0).grow(1.0), spacer(8.0).grow(3.0)], 0.0);
    assert_eq!(
        component.query(Level::Preferred, &renderer).minimum.width,
        12.0
    );
    let mut root = responsive_child(Level::AllPermitted, component);
    let mut tree = Tree::empty();
    let node = root.layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(52.0, 32.0)),
    );
    assert_eq!(node.children()[0].children()[0].size().width, 14.0);
    assert_eq!(node.children()[0].children()[1].size().width, 38.0);
}

#[test]
fn optional_whole_pixel_offers_keep_fractional_content_from_shifting_neighbors() {
    let renderer = renderer();
    let leaf = || child(|_| widget::Space::new().width(10.25).height(32).into());
    let raw: Child<'_, u8> = leaf();
    assert_eq!(
        raw.query(Level::Preferred, &renderer).preferred.width,
        10.25
    );
    let snapped = leaf().round_width_up();
    assert_eq!(
        snapped.query(Level::Preferred, &renderer).preferred.width,
        11.0
    );
    let component = row_child(vec![spacer(0.0), snapped, leaf().round_width_up()], 8.0);
    let mut root = responsive_child(Level::AllPermitted, component);
    let mut tree = Tree::empty();
    let node = root.layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(100.0, 32.0)),
    );
    let boxes = node.children()[0].children();
    assert_eq!(boxes[1].bounds().x, 70.0);
    assert_eq!(boxes[2].bounds().x, 89.0);
}
