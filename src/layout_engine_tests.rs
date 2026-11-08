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

#[test]
fn invalid_query_constraints_never_produce_a_geometry_plan() {
    let renderer = renderer();
    let content: Child<'_, ()> = child(|_| widget::Space::new().width(40).height(20).into());
    for available in [
        Size::new(f32::NAN, 30.0),
        Size::new(60.0, f32::NAN),
        Size::new(-1.0, 30.0),
        Size::new(60.0, -1.0),
        Size::new(f32::NEG_INFINITY, 30.0),
    ] {
        assert_eq!(
            content.query_at(Level::Preferred, &renderer, available),
            Err(AllocationError::InvalidInput)
        );
    }
    assert_eq!(
        content
            .query_at(Level::Preferred, &renderer, Size::INFINITE)
            .unwrap()
            .size,
        Size::new(40.0, 20.0)
    );
}

#[derive(Default)]
struct LayoutCounter(u32);
struct StatefulHeight;

// A native widget whose local state changes its next measured height. Queries
// always receive a fresh counter, while the committed tree retains its counter.
// This deliberately exercises disagreement without any factory side effect.
impl Widget<(), Theme, Renderer> for StatefulHeight {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<LayoutCounter>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(LayoutCounter::default())
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }
    fn layout(&mut self, tree: &mut Tree, _: &Renderer, _: &layout::Limits) -> layout::Node {
        let counter = tree.state.downcast_mut::<LayoutCounter>();
        let height = 20.0 + counter.0 as f32 * 10.0;
        counter.0 += 1;
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
fn commit_rejects_a_native_state_geometry_disagreement_inside_a_shorter_sibling() {
    let renderer = renderer();
    let changing = child(|_| Element::new(StatefulHeight));
    let taller = child(|_| widget::Space::new().width(40).height(100).into());
    let mut root = stages(vec![Stage::new(
        Level::Preferred,
        row_child(vec![changing, taller], 0.0),
    )]);
    let mut tree = Tree::empty();
    let limits = layout::Limits::new(Size::ZERO, Size::new(100.0, 200.0));
    let first = root.layout(&mut tree, &renderer, &limits);
    assert!(root.selection().unwrap().is_ok());
    assert_eq!(first.size().height, 100.0);
    let second = root.layout(&mut tree, &renderer, &limits);
    // Overall row dimensions would still be 80 by 100: checking only the union
    // rectangle misses the first child's changed native height.
    assert!(root.selection().unwrap().is_err());
    assert_eq!(second.children()[0].size(), Size::ZERO);
}

#[test]
fn column_height_growth_expands_a_short_viewport_after_the_fixed_header() {
    let renderer = renderer();
    let header: Child<'_, ()> = child(|_| widget::Space::new().width(80).height(20).into());
    let viewport = scroll_child(
        |_| cosmic::iced::widget::scrollable(widget::Space::new().width(80).height(20)).into(),
        Size::new(80.0, 10.0),
    )
    .grow_height(1.0);
    let column = column_child(vec![header, viewport], 5.0);
    let query = column
        .query_at(Level::Preferred, &renderer, Size::new(80.0, 100.0))
        .unwrap();
    assert_eq!(query.size.height, 100.0);
    assert_eq!(query.children[0].geometry.size.height, 20.0);
    assert_eq!(query.children[1].position.y, 25.0);
    assert_eq!(query.children[1].geometry.size.height, 75.0);
    let mut root = stages(vec![Stage::new(Level::Preferred, column)]);
    let mut tree = Tree::empty();
    let node = root.layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(80.0, 100.0)),
    );
    assert!(root.selection().unwrap().is_ok());
    assert_eq!(node.size().height, 100.0);
}

#[test]
fn live_geometry_disagreement_tries_the_next_declared_stage() {
    let renderer = renderer();
    let first = row_child(
        vec![
            child(|_| Element::new(StatefulHeight)),
            child(|_| widget::Space::new().width(40).height(100).into()),
        ],
        0.0,
    );
    let later = child(|_| widget::Space::new().width(60).height(40).into());
    let mut root = stages(vec![
        Stage::new(Level::Preferred, first),
        Stage::new(Level::Mild, later),
    ]);
    let mut tree = Tree::empty();
    let limits = layout::Limits::new(Size::ZERO, Size::new(100.0, 200.0));
    root.layout(&mut tree, &renderer, &limits);
    assert_eq!(root.selection().unwrap().as_ref().unwrap().stage_index, 0);
    let next = root.layout(&mut tree, &renderer, &limits);
    let selected = root.selection().unwrap().as_ref().unwrap();
    assert_eq!(selected.stage_index, 1);
    assert_eq!(selected.level, Level::Mild);
    assert_eq!(next.children()[0].size(), Size::new(60.0, 40.0));
}

#[test]
fn structured_forms_inherit_continuous_minima_and_forward_the_requested_level() {
    let renderer = renderer();
    let title = child(|_| widget::Space::new().width(100).height(20).into())
        .minimum_width(|level, _| if level >= Level::Mild { 20.0 } else { 60.0 });
    let fixed = child(|_| widget::Space::new().width(40).height(20).into());
    let menu = child(|_| widget::Space::new().width(24).height(20).into());
    let family: Child<'_, ()> = forms(vec![
        (Level::Preferred, row_child(vec![title, fixed], 0.0)),
        (Level::Strong, menu),
    ]);
    assert_eq!(
        family.query(Level::Preferred, &renderer).minimum.width,
        100.0
    );
    assert_eq!(family.query(Level::Mild, &renderer).minimum.width, 60.0);
    assert_eq!(family.query(Level::Strong, &renderer).minimum.width, 24.0);
    assert_eq!(
        family.query(Level::AllPermitted, &renderer).minimum.width,
        24.0
    );
    let trailing = child(|_| widget::Space::new().width(10).height(20).into());
    let outer = row_child(vec![family.weight(3.0), trailing], 0.0);
    assert!(
        outer
            .query_at(Level::Preferred, &renderer, Size::new(90.0, 30.0))
            .is_err()
    );
    let mild = outer
        .query_at(Level::Mild, &renderer, Size::new(90.0, 30.0))
        .unwrap();
    assert_eq!(mild.children[0].geometry.size.width, 80.0);
    assert_eq!(
        mild.children[0].geometry.children[0].geometry.size.width,
        40.0
    );
    let strong = outer
        .query_at(Level::Strong, &renderer, Size::new(90.0, 30.0))
        .unwrap();
    assert_eq!(strong.size.width, 34.0);
    assert!(strong.children[0].geometry.children.is_empty());
    let mut root = stages(vec![
        Stage::new(Level::Preferred, outer.clone()),
        Stage::new(Level::Mild, outer),
    ]);
    let mut tree = Tree::empty();
    root.layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(90.0, 30.0)),
    );
    let selected = root.selection().unwrap().as_ref().unwrap();
    assert_eq!(selected.level, Level::Mild);
    assert_eq!(selected.geometry, mild);
}

#[test]
fn structured_root_preserves_selected_composite_elastic_allocation() {
    let renderer = renderer();
    let make_row = || -> Child<'static, ()> {
        row_child(
            vec![
                child(|_| widget::Space::new().width(40).height(20).into()),
                spacer(0.0),
                child(|_| widget::Space::new().width(32).height(20).into()),
            ],
            0.0,
        )
    };
    let direct = make_row();
    let family = forms(vec![
        (Level::Preferred, make_row()),
        (
            Level::Strong,
            child(|_| widget::Space::new().width(24).height(20).into()),
        ),
    ]);
    let available = Size::new(200.0, 40.0);
    let expected = direct.query_at(Level::Mild, &renderer, available).unwrap();
    let inherited = family.query_at(Level::Mild, &renderer, available).unwrap();
    assert_eq!(inherited, expected);
    assert_eq!(inherited.size.width, 200.0);
    assert_eq!(inherited.children[1].geometry.size.width, 128.0);
    assert_eq!(inherited.children[2].position.x, 168.0);
    // A different selected form can be an ordinary intrinsic native control.
    assert_eq!(
        family
            .query_at(Level::Strong, &renderer, available)
            .unwrap()
            .size
            .width,
        24.0
    );
    let mut direct_root = stages(vec![Stage::new(Level::Mild, direct)]);
    let mut family_root = stages(vec![Stage::new(Level::Mild, family)]);
    let limits = layout::Limits::new(Size::ZERO, available);
    let native_direct = direct_root.layout(&mut Tree::empty(), &renderer, &limits);
    let native_family = family_root.layout(&mut Tree::empty(), &renderer, &limits);
    assert!(direct_root.selection().unwrap().is_ok());
    assert!(family_root.selection().unwrap().is_ok());
    assert_eq!(native_family.size(), native_direct.size());
    let direct_slots = native_direct.children()[0].children();
    let family_slots = native_family.children()[0].children();
    assert_eq!(family_slots.len(), direct_slots.len());
    for (family_slot, direct_slot) in family_slots.iter().zip(direct_slots) {
        assert_eq!(family_slot.bounds(), direct_slot.bounds());
    }
}
