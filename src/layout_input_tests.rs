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

fn search() -> Child<'static, ()> {
    child(|_| {
        widget::text_input::search_input("Search", "")
            .on_input(|_| ())
            .width(Length::Fill)
            .into()
    })
    .minimum_width(|_, _| 180.0)
    .grow(1.0)
}

#[test]
fn native_search_input_uses_its_declared_floor_when_intrinsic_width_is_unbounded() {
    let renderer = renderer();
    let component = search();
    let measured = component.query(Level::Preferred, &renderer);
    assert!(finite_size(measured.preferred));
    assert_eq!(measured.minimum.width, 180.0);
    assert!(measured.preferred.width >= 180.0);
    assert!(
        component
            .query_at(Level::Preferred, &renderer, Size::new(179.0, 100.0))
            .is_err()
    );
    for width in [180.0, 360.0] {
        let geometry = component
            .query_at(Level::Preferred, &renderer, Size::new(width, 100.0))
            .unwrap();
        assert_eq!(geometry.size.width, width);
        assert!(geometry.size.height > 0.0 && geometry.size.height <= 100.0);
        let mut root = stages(vec![Stage::new(Level::Preferred, component.clone())]);
        let node = root.layout(
            &mut Tree::empty(),
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(width, 100.0)),
        );
        assert!(root.selection().unwrap().is_ok());
        assert_eq!(node.children()[0].size(), geometry.size);
    }
}

#[test]
fn nested_header_search_keeps_finite_ranges_and_commits_its_native_input() {
    let renderer = renderer();
    let header = || -> Element<'static, ()> {
        stages(vec![Stage::new(
            Level::Preferred,
            wrap_child(
                vec![
                    search(),
                    child(|_| widget::Space::new().width(80).height(32).into()),
                ],
                8.0,
            ),
        )])
        .into()
    };
    let component = child(move |_| header())
        .minimum_width(|_, _| 180.0)
        .grow(1.0);
    let measured = component.query(Level::Preferred, &renderer);
    assert!(finite_size(measured.preferred));
    let mut root = stages(vec![Stage::new(Level::Preferred, component)]);
    let node = root.layout(
        &mut Tree::empty(),
        &renderer,
        &layout::Limits::new(Size::ZERO, Size::new(300.0, 100.0)),
    );
    assert!(root.selection().unwrap().is_ok());
    assert!(node.size().height > 0.0);
}

#[test]
fn explicit_fill_scroll_viewport_uses_its_finite_minimum_and_commits_after_header() {
    let renderer = renderer();
    let viewport: Child<'_, ()> = scroll_child(
        |_| {
            widget::scrollable(
                widget::column::with_children(vec![
                    widget::text_input::search_input("Search", "")
                        .on_input(|_| ())
                        .width(Length::Fill)
                        .into(),
                    widget::Space::new().width(Length::Fill).height(600).into(),
                ])
                .width(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        },
        Size::new(180.0, 80.0),
    )
    .grow(1.0)
    .grow_height(1.0);
    let measured = viewport.query(Level::Preferred, &renderer);
    assert!(finite_size(measured.preferred));
    assert_eq!(measured.minimum, Size::new(180.0, 80.0));
    assert!(
        viewport
            .query_at(Level::Preferred, &renderer, Size::new(179.0, 100.0))
            .is_err()
    );
    assert!(
        viewport
            .query_at(Level::Preferred, &renderer, Size::new(200.0, 79.0))
            .is_err()
    );
    let component = column_child(
        vec![
            child(|_| widget::Space::new().width(180).height(20).into()),
            viewport,
        ],
        4.0,
    );
    let available = Size::new(320.0, 140.0);
    let geometry = component
        .query_at(Level::Preferred, &renderer, available)
        .unwrap();
    assert_eq!(geometry.size, available);
    assert_eq!(geometry.children[1].geometry.size, Size::new(320.0, 116.0));
    let mut root = stages(vec![Stage::new(Level::Preferred, component)]);
    let node = root.layout(
        &mut Tree::empty(),
        &renderer,
        &layout::Limits::new(Size::ZERO, available),
    );
    assert!(root.selection().unwrap().is_ok());
    assert_eq!(node.size(), available);
}
