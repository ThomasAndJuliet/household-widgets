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
fn decorator_preserves_structured_ranges_and_runs_once_at_the_selected_boundary() {
    let renderer = renderer();
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let component: Child<'_, ()> = forms(vec![(
        Level::Preferred,
        row_child(
            vec![
                child(|_| widget::Space::new().width(100).height(20).into())
                    .minimum_width(|_, _| 40.0),
                child(|_| widget::Space::new().width(20).height(20).into()),
            ],
            0.0,
        ),
    )]);
    let before = component.query(Level::Mild, &renderer);
    let plain = component
        .query_at(Level::Mild, &renderer, Size::new(80.0, 40.0))
        .unwrap();
    let component = component.map_view(move |content| {
        observed.set(observed.get() + 1);
        widget::container(content).into()
    });
    assert_eq!(component.query(Level::Mild, &renderer), before);
    assert_eq!(before.minimum.width, 60.0);
    assert_eq!(
        component
            .query_at(Level::Mild, &renderer, Size::new(80.0, 40.0))
            .unwrap(),
        plain
    );
    assert_eq!(calls.get(), 0);
    let mut root = stages(vec![Stage::new(Level::Mild, component)]);
    let node = root.layout(
        &mut Tree::empty(),
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(80.0, 40.0)),
    );
    assert!(root.selection().unwrap().is_ok());
    assert_eq!(root.selection().unwrap().as_ref().unwrap().geometry, plain);
    assert_eq!(node.children()[0].size(), plain.size);
    assert_eq!(calls.get(), 1);
}

#[test]
fn padding_decorator_is_rejected_even_inside_a_fixed_parent_slot() {
    let renderer = renderer();
    for horizontal_only in [false, true] {
        let invalid: Child<'_, ()> = row_child(
            vec![child(|_| widget::Space::new().width(40).height(20).into())],
            0.0,
        )
        .map_view(move |content| {
            let padding = if horizontal_only {
                cosmic::iced::Padding::new(0.0).left(4).right(4)
            } else {
                cosmic::iced::Padding::new(4.0)
            };
            widget::container(content).padding(padding).into()
        });
        let component = row_child(vec![invalid], 0.0);
        let mut root = stages(vec![Stage::new(Level::Preferred, component)]);
        let node = root.layout(
            &mut Tree::empty(),
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(100.0, 100.0)),
        );
        assert!(
            root.selection().unwrap().is_err(),
            "horizontal_only={horizontal_only}"
        );
        assert_eq!(node.children()[0].size(), Size::ZERO);
    }
}

#[test]
fn direct_decorated_leaf_view_also_rejects_changed_geometry() {
    let renderer = renderer();
    let leaf: Child<'_, ()> = child(|_| widget::Space::new().width(40).height(20).into())
        .map_view(|content| widget::container(content).padding(4).into());
    assert_eq!(
        leaf.query(Level::Preferred, &renderer).preferred,
        Size::new(40.0, 20.0)
    );
    let mut view = leaf.view(Level::Preferred);
    let mut tree = Tree::new(view.as_widget());
    tree.diff(view.as_widget_mut());
    let node = view.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(80.0, 80.0)),
    );
    assert_eq!(node.size(), Size::ZERO);
}

fn settings_row() -> Child<'static, ()> {
    let label = child(|_| {
        widget::column::with_children(vec![
            widget::row::with_children(vec![
                widget::text("Label")
                    .size(14)
                    .line_height(cosmic::iced::widget::text::LineHeight::Absolute(Pixels(21.0)))
                    .into(),
            ])
            .width(Length::Fill)
            .into(),
            widget::text("A longer description with several words that wraps onto many caption lines at the offered width.")
                .size(12)
                .line_height(cosmic::iced::widget::text::LineHeight::Absolute(Pixels(17.0)))
                .width(Length::Fill)
                .into(),
        ])
        .spacing(3)
        .width(Length::Fill)
        .into()
    })
    .minimum_width(|_, _| 40.0)
    .grow(1.0);
    let control = child(|_| {
        widget::container(widget::button::text("Action").on_press(()))
            .width(90)
            .height(32)
            .into()
    })
    .weight(0.0);
    row_child(vec![label, control], 8.0)
}

#[test]
fn shared_settings_alignment_finds_wrapped_caption_and_preserves_the_row_box() {
    let renderer = renderer();
    let available = Size::new(240.0, 400.0);
    let plain = settings_row();
    let aligned =
        settings_row().map_view(|content| crate::Level::new(content, || 21.0).text_first().into());
    let expected = plain
        .query_at(Level::Preferred, &renderer, available)
        .unwrap();
    assert!(expected.size.height > 41.0);
    assert_eq!(
        aligned
            .query_at(Level::Preferred, &renderer, available)
            .unwrap(),
        expected
    );
    let mut plain_root = stages(vec![Stage::new(Level::Preferred, plain)]);
    let mut aligned_root = stages(vec![Stage::new(Level::Preferred, aligned)]);
    let limits = layout::Limits::new(Size::ZERO, available);
    let mut plain_tree = Tree::empty();
    let mut aligned_tree = Tree::empty();
    let plain_node = plain_root.layout(&mut plain_tree, &renderer, &limits);
    let aligned_node = aligned_root.layout(&mut aligned_tree, &renderer, &limits);
    assert!(plain_root.selection().unwrap().is_ok());
    assert!(aligned_root.selection().unwrap().is_ok());
    assert_eq!(aligned_node.size(), plain_node.size());
    assert_eq!(aligned_node.children()[0].size(), expected.size);
    let plain_control = plain_node.children()[0].children()[1].bounds();
    let aligned_control = aligned_node.children()[0].children()[1].bounds();
    assert_eq!(aligned_control.height, 32.0);
    // Inspect the undecorated native label geometry, independently of Level's
    // wrapper-recognition policy. Native shaping may round the requested21px
    // label line; the first caption line is explicitly17px in this fixture.
    let mut label_column = &plain_node.children()[0].children()[0];
    while let [single] = label_column.children() {
        label_column = single;
    }
    let [native_label, native_description] = label_column.children() else {
        panic!("native label and description column was not found");
    };
    let native_label = native_label.bounds();
    let native_description = native_description.bounds();
    let gap = native_description.y - native_label.y - native_label.height;
    assert!((gap - 3.0).abs() < 0.01);
    assert!(
        (20.0..=23.0).contains(&native_label.height),
        "one-line fixture label={native_label:?}, description={native_description:?}"
    );
    let first_band = native_label.height + gap + 17.0_f32.min(native_description.height);
    let expected_y = (first_band.max(aligned_control.height) - aligned_control.height) / 2.0;
    assert!(
        (aligned_control.y - expected_y).abs() < 0.01,
        "control={aligned_control:?}, expected_y={expected_y}, label={native_label:?}, description={native_description:?}"
    );
    assert!(plain_control.y > aligned_control.y);
    #[cfg(feature = "a11y")]
    {
        let plain_nodes = plain_root.a11y_nodes(
            Layout::new(&plain_node),
            &plain_tree,
            mouse::Cursor::Unavailable,
        );
        let aligned_nodes = aligned_root.a11y_nodes(
            Layout::new(&aligned_node),
            &aligned_tree,
            mouse::Cursor::Unavailable,
        );
        assert!(!plain_nodes.root().is_empty());
        assert_eq!(aligned_nodes.root().len(), plain_nodes.root().len());
    }
}

#[test]
fn allocated_fill_label_occupies_its_width_and_keeps_the_declared_useful_floor() {
    let renderer = renderer();
    let row = settings_row();
    let label = row.composition.as_ref().unwrap().children[0].clone();
    let floor = label.clone().minimum_width(|_, _| 160.0);
    let offer = floor.query(Level::Preferred, &renderer);
    assert_eq!(offer.minimum.width, 160.0);
    assert!(offer.preferred.width >= 160.0);
    assert!(
        floor
            .query_at(Level::Preferred, &renderer, Size::new(159.0, 400.0))
            .is_err()
    );

    let geometry = label
        .query_at(Level::Preferred, &renderer, Size::new(142.0, 400.0))
        .unwrap();
    assert_eq!(geometry.size.width, 142.0);
    assert_eq!(geometry.native_size.unwrap().width, 142.0);
    let mut view = label.view(Level::Preferred);
    let mut tree = Tree::new(view.as_widget());
    tree.diff(view.as_widget_mut());
    let node = view.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::with_compression(
            Size::ZERO,
            Size::new(142.0, f32::INFINITY),
            Size::new(false, true),
        ),
    );
    assert_eq!(node.size(), geometry.native_size.unwrap());
    let [native_label_row, native_description] = node.children() else {
        panic!("native setting label and description must remain a column");
    };
    assert_eq!(native_label_row.size().width, 142.0);
    assert_eq!(native_description.size().width, 142.0);
    assert!(
        native_label_row.children()[0].size().width > 0.0,
        "native label text must have usable shaped width"
    );
    assert!(
        native_description.size().height > 17.0,
        "caption must wrap at allocated width"
    );
    // The same text requires less height when the parent supplies more width.
    let wider = label
        .query_at(Level::Preferred, &renderer, Size::new(300.0, 400.0))
        .unwrap();
    assert!(geometry.size.height > wider.size.height);
}
