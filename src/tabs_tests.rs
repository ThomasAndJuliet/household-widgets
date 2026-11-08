// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use cosmic::iced::advanced::renderer::Headless;
use cosmic::iced::advanced::widget::operation::Focusable;
use cosmic::iced::{Font, Pixels, Point, keyboard};

fn renderer() -> Renderer {
    cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
        Font::default(),
        Pixels(14.0),
        Some("tiny-skia"),
    ))
    .expect("native renderer")
}

fn model() -> Model {
    Model::new(
        [
            "Overview of all household machines",
            "Local runners and queued jobs",
            "Services on remote machines",
            "Continuous integration pipelines",
        ]
        .into_iter()
        .map(|label| Tab {
            label: label.into(),
            icon: widget::icon::from_name("computer-symbolic").handle(),
        })
        .collect(),
        0,
    )
}

fn index(index: usize) -> usize {
    index
}

fn laid_out<'a>(
    component: Child<'a, usize>,
    level: Level,
    renderer: &Renderer,
    width: f32,
    mut tree: Tree,
) -> (Element<'a, usize>, Tree, layout::Node) {
    let mut element = component.view(level);
    tree.diff(element.as_widget_mut());
    let node = element.as_widget_mut().layout(
        &mut tree,
        renderer,
        &layout::Limits::new(Size::ZERO, Size::new(width, 100.0)),
    );
    (element, tree, node)
}

#[test]
fn intrinsic_native_sizes_and_viewport_minima_follow_policy_and_font() {
    let renderer = renderer();
    let mut previous: Option<(f32, f32)> = None;
    for font_size in [14.0, 22.0] {
        let model = model().font_size(font_size).button_height(48);
        let dropdown = strip(&model, Overflow::Dropdown, index);
        let full = dropdown.query(Level::Preferred, &renderer);
        let mild = dropdown.query(Level::Mild, &renderer);
        let compact = dropdown.query(Level::Strong, &renderer);
        let collapsed = dropdown.query(Level::AllPermitted, &renderer);
        if let Some((full_width, collapsed_width)) = previous {
            assert!(full.preferred.width > full_width);
            assert!(collapsed.preferred.width > collapsed_width);
        }
        previous = Some((full.preferred.width, collapsed.preferred.width));
        assert_eq!(full, mild);
        assert!(full.preferred.width > compact.preferred.width);
        assert_eq!(full.preferred.height, 48.0);
        assert_eq!(full.minimum, full.preferred);
        let scroll = strip(
            &model,
            Overflow::Scroll {
                minimum_viewport: 100.0,
            },
            index,
        );
        let viewport = scroll.query(Level::Mild, &renderer);
        assert_eq!(viewport.minimum.width, 100.0);
        assert!((viewport.preferred.width - full.preferred.width).abs() < 0.5);
        let (_, _, node) = laid_out(
            dropdown,
            Level::Preferred,
            &renderer,
            full.preferred.width,
            Tree::empty(),
        );
        assert!((node.size().width - full.preferred.width).abs() < 0.5);
    }
}

#[test]
fn dropdown_query_honours_the_native_header_height_contract() {
    let renderer = renderer();
    let model = model().button_height(32);
    let component = strip(&model, Overflow::Dropdown, index);
    let measured = component.query(Level::AllPermitted, &renderer);
    assert_eq!(measured.preferred.height, 32.0);
    assert_eq!(measured.minimum.height, 32.0);
    let (_, _, node) = laid_out(
        component,
        Level::AllPermitted,
        &renderer,
        measured.preferred.width,
        Tree::empty(),
    );
    assert_eq!(node.size().height, 32.0);
}

#[test]
fn compact_inactive_hover_keeps_its_full_name_in_the_native_tooltip() {
    let renderer = renderer();
    let model = model().button_height(32);
    let (mut element, mut tree, node) = laid_out(
        strip(&model, Overflow::Dropdown, index),
        Level::Strong,
        &renderer,
        2000.0,
        Tree::empty(),
    );
    let range = model.ranges(true, &renderer)[2];
    let at = Point::new((range.0 + range.1) / 2.0, 8.0);
    let event = Event::Mouse(mouse::Event::CursorMoved { position: at });
    element.as_widget_mut().update(
        &mut tree,
        &event,
        Layout::new(&node),
        mouse::Cursor::Available(at),
        &renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut Shell::new(&mut Vec::new()),
        &Rectangle::with_size(Size::new(2000.0, 100.0)),
    );
    assert_eq!(tree.state.downcast_ref::<HoverState>().hovered, Some(2));
    let viewport = Rectangle::with_size(Size::new(2000.0, 100.0));
    let mut overlay = element
        .as_widget_mut()
        .overlay(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &viewport,
            Vector::ZERO,
        )
        .expect("hovered compact tab has a native tooltip");
    let group = overlay.as_overlay_mut().layout(&renderer, viewport.size());
    assert_eq!(
        group.children().len(),
        1,
        "only the tooltip overlay is open"
    );
    let bubble = &group.children()[0];
    // Native tooltip overlays implement layout/draw, but no Operation traversal.
    // Compare the actual overlay geometry with the full-name native bubble.
    let expected = measure_element::<usize>(
        || crate::tip_bubble(widget::text(model.items[2].label.clone())).into(),
        &renderer,
    );
    assert!(
        (bubble.size().width - expected.width).abs() < 0.5,
        "actual={:?}, expected={expected:?}",
        bubble.size()
    );
    assert!((bubble.size().height - expected.height).abs() < 0.5);
}

#[derive(Default, Debug)]
struct ScrollPosition {
    offset: f32,
    viewport: f32,
    content: f32,
    viewport_height: f32,
    content_height: f32,
}
impl Operation for ScrollPosition {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn scrollable(
        &mut self,
        _: Option<&cosmic::iced::advanced::widget::Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        _: &mut dyn cosmic::iced::advanced::widget::operation::Scrollable,
    ) {
        self.offset = translation.x;
        self.viewport = bounds.width;
        self.content = content.width;
        self.viewport_height = bounds.height;
        self.content_height = content.height;
    }
}

#[test]
fn scrolling_tabs_reserve_a_separate_scrollbar_lane() {
    let renderer = renderer();
    for (height, font) in [(32, 14.0), (48, 22.0)] {
        let model = model().button_height(height).font_size(font);
        let component = strip(
            &model,
            Overflow::Scroll {
                minimum_viewport: 100.0,
            },
            index,
        );
        let measured = component.query(Level::Mild, &renderer);
        let (mut element, mut tree, node) =
            laid_out(component, Level::Mild, &renderer, 160.0, Tree::empty());
        let mut position = ScrollPosition::default();
        element
            .as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut position);
        assert!(position.content > position.viewport);
        assert_eq!(
            position.content_height,
            f32::from(height),
            "tabs keep their full height"
        );
        assert!(
            position.viewport_height - position.content_height >= 10.0,
            "scrollbar needs an 8px lane plus 2px gap outside tabs: {position:?}"
        );
        assert_eq!(measured.minimum.height, node.size().height);
    }
}

#[test]
fn selecting_the_last_tab_reveals_it_without_equating_viewport_and_content() {
    let renderer = renderer();
    let mut model = model().font_size(20.0).button_height(48);
    let policy = Overflow::Scroll {
        minimum_viewport: 100.0,
    };
    let (_, tree, _) = laid_out(
        strip(&model, policy, index),
        Level::Mild,
        &renderer,
        160.0,
        Tree::empty(),
    );
    model.select(3);
    assert_eq!(model.full.active(), model.full_ids[3]);
    assert_eq!(model.compact.active(), model.compact_ids[3]);
    assert!(model.compact.text(model.compact_ids[0]).is_none());
    assert_eq!(
        model.compact.text(model.compact_ids[3]),
        Some(model.items[3].label.as_str())
    );
    let (mut element, mut tree, node) = laid_out(
        strip(&model, policy, index),
        Level::Mild,
        &renderer,
        160.0,
        tree,
    );
    let mut position = ScrollPosition::default();
    element
        .as_widget_mut()
        .operate(&mut tree, Layout::new(&node), &renderer, &mut position);
    let (start, end) = model.selected_range(&renderer);
    assert!(position.content > position.viewport);
    assert!(position.offset > 0.0);
    assert!(start + 0.5 >= position.offset);
    assert!(end <= position.offset + position.viewport + 0.5 || end - start > position.viewport);
}

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

fn key(key: keyboard::key::Named, released: bool) -> Event {
    let key = keyboard::Key::Named(key);
    let physical_key = keyboard::key::Physical::Code(keyboard::key::Code::Enter);
    if released {
        Event::Keyboard(keyboard::Event::KeyReleased {
            key: key.clone(),
            modified_key: key,
            physical_key,
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
        })
    } else {
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key,
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        })
    }
}

#[test]
fn native_tab_keyboard_activation_reaches_the_same_index_callback() {
    let renderer = renderer();
    let model = model();
    let (mut element, mut tree, node) = laid_out(
        strip(&model, Overflow::Dropdown, index),
        Level::Preferred,
        &renderer,
        2000.0,
        Tree::empty(),
    );
    element
        .as_widget_mut()
        .operate(&mut tree, Layout::new(&node), &renderer, &mut Focus);
    let viewport = Rectangle::with_size(Size::new(2000.0, 100.0));
    let mut messages = Vec::new();
    for event in [
        key(keyboard::key::Named::Enter, false),
        key(keyboard::key::Named::Enter, true),
    ] {
        element.as_widget_mut().update(
            &mut tree,
            &event,
            Layout::new(&node),
            mouse::Cursor::Available(Point::new(8.0, 8.0)),
            &renderer,
            &mut cosmic::iced::advanced::clipboard::Null,
            &mut Shell::new(&mut messages),
            &viewport,
        );
    }
    assert_eq!(messages, vec![0]);
    // The same native segmented control survives a parent changing its level.
    let (mut element, mut tree, node) = laid_out(
        strip(&model, Overflow::Dropdown, index),
        Level::Strong,
        &renderer,
        2000.0,
        tree,
    );
    let mut messages = Vec::new();
    element.as_widget_mut().update(
        &mut tree,
        &key(keyboard::key::Named::Enter, true),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &renderer,
        &mut cosmic::iced::advanced::clipboard::Null,
        &mut Shell::new(&mut messages),
        &viewport,
    );
    assert_eq!(messages, vec![0]);
}
