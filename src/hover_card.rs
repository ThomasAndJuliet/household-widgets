// SPDX-License-Identifier: GPL-3.0-only

//! Informational hover attached to a live native source.
use crate::layout::{AnchorCandidate, Attachment, HoverRetained, Stage, commit_hover_retaining};
use crate::popup::Anchors;
use cosmic::iced::advanced::widget::{Id, Operation, Tree};
use cosmic::iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use cosmic::iced::{Event, Length, Rectangle, Size, Vector};
use cosmic::{Element, Renderer, Theme, widget};

type ReportPreparation<'a, Message> =
    dyn Fn(Option<crate::layout::SourceReport>) -> crate::layout::Child<'a, Message> + 'a;

/// The informational surface for engine-owned hover contents. Unlike the
/// legacy tooltip wrapper, these overlays must draw their own background.
pub fn card<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> widget::Container<'a, Message, Theme> {
    crate::tip_bubble(content).class(crate::popup_surface::style())
}

/// A readable lower bound for a wrapping explanation, capped by its natural width.
pub const TEXT_MIN_WIDTH: f32 = 120.0;

/// Prepare ordinary wrapping text. Emphasis changes the text weight; verbose
/// is unsupported because plain text has no additional prepared detail.
pub fn text_box<'a, Message: 'a>(
    text: String,
    instance: crate::layout::PresentationRequest,
    defaults: crate::layout::PresentationRequest,
) -> Result<crate::layout::Child<'a, Message>, crate::layout::UnsupportedPresentation> {
    text_box_with(text, instance, defaults, |text, emphasis| {
        widget::text(text)
            .font(if emphasis == crate::layout::Emphasis::Emphasised {
                cosmic::font::semibold()
            } else {
                cosmic::font::default()
            })
            .wrapping(cosmic::iced::widget::text::Wrapping::WordOrGlyph)
            .into()
    })
}

/// Prepare text using the caller's interface typography. The factory must
/// honour both supported emphasis values and produce wrapping text.
pub fn text_box_with<'a, Message: 'a>(
    text: String,
    instance: crate::layout::PresentationRequest,
    defaults: crate::layout::PresentationRequest,
    render: impl Fn(String, crate::layout::Emphasis) -> Element<'a, Message> + 'a,
) -> Result<crate::layout::Child<'a, Message>, crate::layout::UnsupportedPresentation> {
    use crate::layout::{self, Detail, Emphasis, Presentation, PresentationFamily};
    let normal = Presentation::default();
    let emphasised = Presentation {
        emphasis: Emphasis::Emphasised,
        detail: Detail::Concise,
    };
    PresentationFamily::new(normal, vec![normal, emphasised]).prepare(
        instance,
        defaults,
        move |presentation| {
            layout::child(move |_| {
                widget::container(render(text.clone(), presentation.emphasis))
                    .max_width(crate::TIP_WIDTH)
                    .into()
            })
            .minimum_width(|_, preferred| preferred.width.min(TEXT_MIN_WIDTH))
        },
    )
}

/// Prepare an existing normal-only text renderer with its own typography.
pub fn normal_text_box_with<'a, Message: 'a>(
    text: String,
    instance: crate::layout::PresentationRequest,
    defaults: crate::layout::PresentationRequest,
    render: impl Fn(String) -> Element<'a, Message> + 'a,
) -> Result<crate::layout::Child<'a, Message>, crate::layout::UnsupportedPresentation> {
    let normal = crate::layout::Presentation::default();
    crate::layout::PresentationFamily::new(normal, vec![normal]).prepare(
        instance,
        defaults,
        move |_| {
            crate::layout::child(move |_| {
                widget::container(render(text.clone()))
                    .max_width(crate::TIP_WIDTH)
                    .into()
            })
            .minimum_width(|_, preferred| preferred.width.min(TEXT_MIN_WIDTH))
        },
    )
}

fn surface_box<'a, Message: 'a>(
    content: crate::layout::Child<'a, Message>,
) -> crate::layout::Child<'a, Message> {
    let surface = crate::layout::framed_child(content, crate::tip_padding(), |content| {
        card(content).into()
    });
    crate::popup_surface::reserve_shadow(surface)
}

/// Attach a single prepared box, measuring the common surface together with it.
pub fn hover_box<'a, Message: 'a>(
    anchors: Anchors,
    source: impl Into<Id>,
    target: impl Into<Element<'a, Message>>,
    content: crate::layout::Child<'a, Message>,
    candidates: Vec<AnchorCandidate>,
) -> Element<'a, Message> {
    let framed = surface_box(content);
    hover(
        anchors,
        source,
        target,
        vec![Stage::new(crate::layout::Level::Preferred, framed)],
        candidates,
    )
}

/// Prepare hover contents from the current accepted source report. The target
/// should register its prepared source with `Anchors::reported_anchor`, whose
/// expected revision filters the report. `None` explicitly means no current
/// matching report; it does not mean that all source information is visible.
///
/// Preparation runs after native source traversal and before hover measurement,
/// once per visible hover layout. It must use only its report snapshot and
/// already prepared application data: no I/O, generation or source-state edits.
/// The returned box is then immutable throughout all candidate queries and its
/// verified commit. Contents, conditions and ordering remain caller policy.
/// An empty/hidden box can deliberately omit the hover.
pub fn hover_box_with_report<'a, Message: 'a>(
    anchors: Anchors,
    source: impl Into<Id>,
    target: impl Into<Element<'a, Message>>,
    prepare: impl Fn(Option<crate::layout::SourceReport>) -> crate::layout::Child<'a, Message> + 'a,
    candidates: Vec<AnchorCandidate>,
) -> Element<'a, Message> {
    hover_with_preparation(
        anchors,
        source.into(),
        target.into(),
        Vec::new(),
        Some(Box::new(prepare)),
        candidates,
    )
}

/// Wrap any target, including a disabled control. The source ID must remain
/// stable across view rebuilds. The caller supplies every allowed direction.
pub fn hover<'a, Message: 'a>(
    anchors: Anchors,
    source: impl Into<Id>,
    target: impl Into<Element<'a, Message>>,
    stages: Vec<Stage<'a, Message>>,
    candidates: Vec<AnchorCandidate>,
) -> Element<'a, Message> {
    hover_with_preparation(
        anchors,
        source.into(),
        target.into(),
        stages,
        None,
        candidates,
    )
}

fn hover_with_preparation<'a, Message: 'a>(
    anchors: Anchors,
    source: Id,
    target: Element<'a, Message>,
    stages: Vec<Stage<'a, Message>>,
    prepare: Option<Box<ReportPreparation<'a, Message>>>,
    candidates: Vec<AnchorCandidate>,
) -> Element<'a, Message> {
    let card = Hover {
        content: target,
        source: source.clone(),
        anchors: anchors.clone(),
        stages,
        prepare,
        candidates,
        body: widget::space().width(0).height(0).into(),
        retained: HoverRetained::default(),
    };
    anchors.anchor(source, Element::new(card))
}

struct Hover<'a, Message> {
    content: Element<'a, Message>,
    source: Id,
    anchors: Anchors,
    stages: Vec<Stage<'a, Message>>,
    prepare: Option<Box<ReportPreparation<'a, Message>>>,
    candidates: Vec<AnchorCandidate>,
    body: Element<'a, Message>,
    retained: HoverRetained,
}
impl<'a, Message: 'a> Widget<Message, Theme, Renderer> for Hover<'a, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![
            Tree::new(self.content.as_widget()),
            Tree::new(self.body.as_widget()),
        ]
    }
    fn diff(&mut self, tree: &mut Tree) {
        tree.children[0].diff(self.content.as_widget_mut());
        self.retained.clear();
    }
    forward_widget!(Message, Theme, Renderer; size, layout, draw, operate, mouse_interaction, drag_destinations);
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
        let was = self.anchors.hover_is_owner(&self.source);
        // A redraw, layout or keystroke cannot let a stationary pointer take
        // ownership back from a deliberate keyboard target.
        if matches!(event, Event::Mouse(mouse::Event::CursorMoved { .. })) {
            if cursor.is_over(layout.bounds())
                && cursor.position().is_some_and(|p| viewport.contains(p))
            {
                self.anchors.claim_hover(&self.source);
            } else {
                self.anchors.release_hover(&self.source);
            }
        } else if matches!(event, Event::Mouse(mouse::Event::CursorLeft)) {
            self.anchors.release_hover(&self.source);
        } else if matches!(event, Event::Window(cosmic::iced::window::Event::Unfocused)) {
            self.anchors.deactivate();
        }
        if was != self.anchors.hover_is_owner(&self.source) {
            shell.invalidate_layout();
            shell.request_redraw();
        }
        self.anchors.update_timer(event, shell);
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let mut focus = crate::popup::FocusProbe::default();
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, &mut focus);
        self.anchors.register_information(&self.source, focus.ids);
        let (target_tree, body_tree) = tree.children.split_at_mut(1);
        let target = self.content.as_widget_mut().overlay(
            &mut target_tree[0],
            layout,
            renderer,
            viewport,
            translation,
        );
        if !self.anchors.information_visible(&self.source) {
            return target;
        }
        let card = overlay::Element::new(Box::new(Card {
            anchors: self.anchors.clone(),
            source: self.source.clone(),
            stages: &mut self.stages,
            prepare: self.prepare.as_deref(),
            candidates: &self.candidates,
            body: &mut self.body,
            tree: &mut body_tree[0],
            retained: &mut self.retained,
        }));
        Some(match target {
            Some(target) => overlay::Group::with_children(vec![target, card]).overlay(),
            None => card,
        })
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
struct Card<'b, 'a, Message> {
    anchors: Anchors,
    source: Id,
    stages: &'b mut Vec<Stage<'a, Message>>,
    prepare: Option<&'b ReportPreparation<'a, Message>>,
    candidates: &'b [AnchorCandidate],
    body: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    retained: &'b mut HoverRetained,
}
impl<'a, Message: 'a> overlay::Overlay<Message, Theme, Renderer> for Card<'_, 'a, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let window = Rectangle::with_size(bounds);
        let Some(anchor) = self
            .anchors
            .visible(&self.source)
            .filter(|_| self.anchors.information_visible(&self.source))
        else {
            self.retained.clear();
            return layout::Node::with_children(bounds, vec![]);
        };
        if let Some(prepare) = self.prepare {
            // Snapshot only after the accepted target has published its report.
            // Candidate geometry queries see the resulting prepared Child and
            // never read the mutable source registry or invoke this factory.
            let report = self.anchors.information(&self.source);
            let content = prepare(report);
            self.retained.clear();
            *self.stages = vec![Stage::new(
                crate::layout::Level::Preferred,
                surface_box(content),
            )];
        }
        let attachment = Attachment::new(anchor, self.candidates.to_vec());
        let children = commit_hover_retaining(
            self.stages,
            renderer,
            window,
            &attachment,
            &self.anchors.surfaces(),
            self.body,
            self.tree,
            self.retained,
        )
        .ok()
        .map(|(_, node)| vec![node])
        .unwrap_or_default();
        layout::Node::with_children(bounds, children)
    }
    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
    ) {
        if let Some(child) = layout.children().next() {
            self.body.as_widget().draw(
                self.tree,
                renderer,
                theme,
                style,
                child,
                mouse::Cursor::Unavailable,
                &layout.bounds(),
            );
        }
    }
    fn index(&self) -> f32 {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Alignment, Level, Side, child};
    use cosmic::iced::advanced::renderer::Headless;
    use cosmic::iced::runtime::{UserInterface, user_interface};
    use cosmic::iced::{Font, Pixels, Point, window};

    #[test]
    fn disabled_target_can_claim_hover_without_capturing_a_click() {
        let mut renderer = cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
            Font::default(),
            Pixels(14.0),
            Some("tiny-skia"),
        ))
        .expect("headless renderer");
        let anchors = Anchors::default();
        let id = Id::unique();
        let target = cosmic::iced::widget::button(cosmic::iced::widget::text("Disabled"));
        let card = hover(
            anchors.clone(),
            id.clone(),
            target,
            vec![Stage::new(
                Level::Preferred,
                child(|_| widget::text("Explanation").into()),
            )],
            vec![AnchorCandidate::new(Side::Below, Alignment::Start)],
        );
        let mut ui = UserInterface::build(
            crate::popup::host(card).anchors(anchors.clone()),
            Size::new(300.0, 200.0),
            user_interface::Cache::new(),
            &mut renderer,
        );
        let cursor = mouse::Cursor::Available(Point::new(5.0, 5.0));
        let mut clipboard = cosmic::iced::advanced::clipboard::Null;
        let mut messages: Vec<()> = Vec::new();
        ui.update(
            &[Event::Window(window::Event::RedrawRequested(
                std::time::Instant::now(),
            ))],
            cursor,
            &mut renderer,
            &mut clipboard,
            &mut messages,
        );
        ui.update(
            &[Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(5.0, 5.0),
            })],
            cursor,
            &mut renderer,
            &mut clipboard,
            &mut messages,
        );
        assert_eq!(anchors.hovered(), Some(id));
        let (_, statuses) = ui.update(
            &[Event::Mouse(mouse::Event::ButtonPressed(
                mouse::Button::Left,
            ))],
            cursor,
            &mut renderer,
            &mut clipboard,
            &mut messages,
        );
        assert!(
            statuses
                .iter()
                .all(|status| *status == cosmic::iced::event::Status::Ignored)
        );
        assert!(messages.is_empty());
    }
    #[test]
    fn moving_between_sources_transfers_the_single_owner() {
        let mut renderer = cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
            Font::default(),
            Pixels(14.0),
            Some("tiny-skia"),
        ))
        .expect("headless renderer");
        let anchors = Anchors::default();
        let first = Id::unique();
        let second = Id::unique();
        let make = |id: Id| {
            hover(
                anchors.clone(),
                id,
                widget::space().width(80).height(40),
                vec![Stage::new(
                    Level::Preferred,
                    child(|_| widget::text("Explanation").into()),
                )],
                vec![AnchorCandidate::new(Side::Below, Alignment::Start)],
            )
        };
        let row = widget::row::with_children(vec![make(first.clone()), make(second.clone())]);
        let mut ui = UserInterface::build(
            crate::popup::host(row).anchors(anchors.clone()),
            Size::new(300.0, 200.0),
            user_interface::Cache::new(),
            &mut renderer,
        );
        let mut clipboard = cosmic::iced::advanced::clipboard::Null;
        let mut messages: Vec<()> = Vec::new();
        let p = Point::new(5.0, 5.0);
        let cursor = mouse::Cursor::Available(p);
        ui.update(
            &[Event::Window(window::Event::RedrawRequested(
                std::time::Instant::now(),
            ))],
            cursor,
            &mut renderer,
            &mut clipboard,
            &mut messages,
        );
        ui.update(
            &[Event::Mouse(mouse::Event::CursorMoved { position: p })],
            cursor,
            &mut renderer,
            &mut clipboard,
            &mut messages,
        );
        assert_eq!(anchors.hovered(), Some(first));
        let p = Point::new(85.0, 5.0);
        ui.update(
            &[Event::Mouse(mouse::Event::CursorMoved { position: p })],
            mouse::Cursor::Available(p),
            &mut renderer,
            &mut clipboard,
            &mut messages,
        );
        assert_eq!(anchors.hovered(), Some(second));
    }

    #[test]
    fn opening_the_source_suppresses_its_hover() {
        let mut renderer = cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
            Font::default(),
            Pixels(14.0),
            Some("tiny-skia"),
        ))
        .expect("headless renderer");
        let anchors = Anchors::default();
        let id = Id::unique();
        let target = hover(
            anchors.clone(),
            id.clone(),
            widget::space().width(80).height(40),
            vec![Stage::new(
                Level::Preferred,
                child(|_| widget::text("Explanation").into()),
            )],
            vec![AnchorCandidate::new(Side::Below, Alignment::Start)],
        );
        let panel = crate::popup::Panel::new(
            Attachment::below(Rectangle::with_size(Size::new(80.0, 40.0))),
            vec![Stage::new(
                Level::Preferred,
                child(|_| widget::space().width(80).height(40).into()),
            )],
        )
        .source(id.clone());
        let mut ui = UserInterface::build(
            crate::popup::host(target)
                .anchors(anchors.clone())
                .chain(vec![panel]),
            Size::new(300.0, 200.0),
            user_interface::Cache::new(),
            &mut renderer,
        );
        let mut clipboard = cosmic::iced::advanced::clipboard::Null;
        let mut messages: Vec<()> = Vec::new();
        let p = Point::new(5.0, 5.0);
        let cursor = mouse::Cursor::Available(p);
        ui.update(
            &[
                Event::Window(window::Event::RedrawRequested(std::time::Instant::now())),
                Event::Mouse(mouse::Event::CursorMoved { position: p }),
            ],
            cursor,
            &mut renderer,
            &mut clipboard,
            &mut messages,
        );
        assert!(anchors.is_open(&id));
        assert_eq!(anchors.hovered(), None);
    }
    #[test]
    fn informational_overlay_does_not_expose_focus_operations() {
        struct Probe(usize);
        impl Operation for Probe {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                self.0 += 1;
                operate(self);
            }
            fn focusable(
                &mut self,
                _: Option<&Id>,
                _: Rectangle,
                _: &mut dyn cosmic::iced::advanced::widget::operation::Focusable,
            ) {
                self.0 += 1;
            }
        }
        let mut renderer = cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
            Font::default(),
            Pixels(14.0),
            Some("tiny-skia"),
        ))
        .expect("headless renderer");
        let anchors = Anchors::default();
        let id = Id::unique();
        let mut stages: Vec<Stage<'_, ()>> = Vec::new();
        let mut body: Element<'_, ()> = cosmic::iced::widget::text_input("", "").into();
        let mut tree = Tree::new(body.as_widget());
        let mut retained = HoverRetained::default();
        let mut card = Card {
            anchors,
            source: id,
            stages: &mut stages,
            prepare: None,
            candidates: &[],
            body: &mut body,
            tree: &mut tree,
            retained: &mut retained,
        };
        let node = layout::Node::new(Size::new(100.0, 100.0));
        let mut probe = Probe(0);
        overlay::Overlay::operate(&mut card, Layout::new(&node), &renderer, &mut probe);
        assert_eq!(probe.0, 0);
        let _ = &mut renderer;
    }
}

#[cfg(test)]
mod text_presentation_tests {
    use super::*;
    use crate::layout::{Detail, Emphasis, PresentationRequest};

    #[test]
    fn text_resolves_defaults_and_rejects_unprepared_detail() {
        let defaults = PresentationRequest {
            emphasis: Some(Emphasis::Emphasised),
            detail: None,
        };
        let text = text_box::<()>(
            "Explanation".into(),
            PresentationRequest::default(),
            defaults,
        )
        .unwrap();
        assert_eq!(text.presentation().unwrap().emphasis, Emphasis::Emphasised);
        let normal = PresentationRequest {
            emphasis: Some(Emphasis::Normal),
            detail: None,
        };
        let text = text_box::<()>("Explanation".into(), normal, defaults).unwrap();
        assert_eq!(text.presentation().unwrap().emphasis, Emphasis::Normal);
        assert!(
            text_box::<()>(
                "Explanation".into(),
                PresentationRequest {
                    emphasis: None,
                    detail: Some(Detail::Verbose)
                },
                defaults
            )
            .is_err()
        );
    }
    #[test]
    fn wrapping_text_resolves_and_commits_the_same_narrow_geometry() {
        use cosmic::iced::advanced::{
            Widget, layout as native_layout, renderer::Headless, widget::Tree,
        };
        use cosmic::iced::{Font, Pixels, Size};
        let renderer = cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
            Font::default(),
            Pixels(14.0),
            Some("tiny-skia"),
        ))
        .expect("headless renderer");
        let content: crate::layout::Child<'_, ()> = text_box(
            "A full explanation with enough words to wrap into several lines at a narrow width."
                .into(),
            PresentationRequest::default(),
            PresentationRequest::default(),
        )
        .unwrap();
        let wide = content
            .query_at(
                crate::layout::Level::Preferred,
                &renderer,
                Size::new(320.0, 500.0),
            )
            .unwrap();
        let narrow = content
            .query_at(
                crate::layout::Level::Preferred,
                &renderer,
                Size::new(140.0, 500.0),
            )
            .unwrap();
        assert!(narrow.size.height > wide.size.height);
        assert!(
            content
                .query_at(
                    crate::layout::Level::Preferred,
                    &renderer,
                    Size::new(140.0, 10.0)
                )
                .is_err()
        );
        let mut root =
            crate::layout::stages(vec![Stage::new(crate::layout::Level::Preferred, content)]);
        let mut tree = Tree::empty();
        let node = root.layout(
            &mut tree,
            &renderer,
            &native_layout::Limits::new(Size::ZERO, Size::new(140.0, 500.0)),
        );
        assert_eq!(root.selection().unwrap().as_ref().unwrap().geometry, narrow);
        assert_eq!(node.children()[0].size(), narrow.size);
    }
    #[test]
    fn surface_preserves_nested_minimum_and_reserves_padding_and_shadow() {
        use cosmic::iced::advanced::{
            Widget, layout as native_layout, renderer::Headless, widget::Tree,
        };
        use cosmic::iced::{Font, Pixels, Size};
        let mut renderer = cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
            Font::default(),
            Pixels(14.0),
            Some("tiny-skia"),
        ))
        .expect("headless renderer");
        let content: crate::layout::Child<'_, ()> = crate::layout::row_child(
            vec![
                crate::layout::child(|_| widget::space().width(100).height(20).into())
                    .minimum_width(|_, _| 40.0),
                crate::layout::child(|_| widget::space().width(20).height(20).into()),
            ],
            0.0,
        );
        let surface = surface_box(content);
        let padding = crate::tip_padding();
        let expected_min = 60.0 + padding.left + padding.right + 16.0;
        let measured = surface.query(crate::layout::Level::Preferred, &renderer);
        assert_eq!(measured.minimum.width, expected_min);
        assert_eq!(
            measured.preferred.height,
            20.0 + padding.top + padding.bottom + 16.0
        );
        assert!(
            surface
                .query_at(
                    crate::layout::Level::Preferred,
                    &renderer,
                    Size::new(expected_min - 1.0, 200.0)
                )
                .is_err()
        );
        let offer = Size::new(expected_min + 20.0, 200.0);
        let expected = surface
            .query_at(crate::layout::Level::Preferred, &renderer, offer)
            .unwrap();
        let mut root =
            crate::layout::stages(vec![Stage::new(crate::layout::Level::Preferred, surface)]);
        let mut tree = Tree::empty();
        let node = root.layout(
            &mut tree,
            &renderer,
            &native_layout::Limits::new(Size::ZERO, offer),
        );
        assert_eq!(
            root.selection().unwrap().as_ref().unwrap().geometry,
            expected
        );
        assert_eq!(node.children()[0].size(), expected.size);
        root.draw(
            &tree,
            &mut renderer,
            &cosmic::Theme::light(),
            &cosmic::iced::advanced::renderer::Style::default(),
            cosmic::iced::advanced::Layout::new(&node),
            cosmic::iced::advanced::mouse::Cursor::Unavailable,
            &Rectangle::with_size(offer),
        );
    }
}

#[cfg(test)]
mod report_preparation_tests {
    use super::*;
    use crate::layout::{
        self, Alignment, InformationId, InformationLoss, Level, LossKind, Side, SourceId,
        SourceReport,
    };
    use cosmic::iced::advanced::renderer::Headless;
    use cosmic::iced::{Font, Pixels};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Inputs = Rc<RefCell<Vec<Option<SourceReport>>>>;

    #[derive(Clone, Copy)]
    struct Snapshot {
        revision: u64,
        expected_revision: u64,
        full: &'static str,
        extra: &'static str,
    }

    fn renderer() -> Renderer {
        cosmic::iced::futures::executor::block_on(<Renderer as Headless>::new(
            Font::default(),
            Pixels(14.0),
            Some("tiny-skia"),
        ))
        .expect("headless renderer")
    }

    fn source_target(anchors: &Anchors, id: Id, snapshot: Snapshot) -> Element<'static, ()> {
        let content = layout::child(|level| {
            widget::space()
                .width(if level == Level::Preferred {
                    260.0
                } else {
                    50.0
                })
                .height(20.0)
                .into()
        })
        .report_information(SourceId(71), snapshot.revision, |level, _| {
            if level == Level::Preferred {
                Vec::new()
            } else {
                vec![InformationLoss {
                    identity: InformationId("name"),
                    kind: LossKind::Omitted,
                }]
            }
        });
        anchors.reported_anchor(
            id,
            SourceId(71),
            snapshot.expected_revision,
            layout::stages(vec![
                Stage::new(Level::Preferred, content.clone()),
                Stage::new(Level::Mild, content),
            ]),
        )
    }

    fn prepare(snapshot: Snapshot, inputs: Inputs) -> Box<ReportPreparation<'static, ()>> {
        Box::new(move |report| {
            inputs.borrow_mut().push(report.clone());
            // This is application composition policy. The host does not know
            // what these identities or independently included contents mean.
            let mut parts = Vec::new();
            if let Some(report) = &report {
                if report
                    .losses
                    .iter()
                    .any(|loss| loss.identity == InformationId("name"))
                {
                    parts.push(layout::child(move |_| widget::text(snapshot.full).into()));
                }
            } else {
                parts.push(layout::child(|_| widget::text("Unknown report").into()));
            }
            parts.push(layout::child(move |_| widget::text(snapshot.extra).into()));
            layout::column_child(parts, 3.0)
        })
    }

    fn hover(
        anchors: &Anchors,
        source: Id,
        snapshot: Snapshot,
        inputs: Inputs,
    ) -> Hover<'static, ()> {
        Hover {
            content: source_target(anchors, source.clone(), snapshot),
            source,
            anchors: anchors.clone(),
            stages: Vec::new(),
            prepare: Some(prepare(snapshot, inputs)),
            candidates: vec![AnchorCandidate::new(Side::Below, Alignment::StartBounded)],
            body: widget::space().into(),
            retained: HoverRetained::default(),
        }
    }

    fn accepted_overlay(
        hover: &mut Hover<'_, ()>,
        tree: &mut Tree,
        renderer: &Renderer,
        size: Size,
    ) -> Option<cosmic::iced::advanced::layout::Node> {
        // These are the same native phases used by Host: accepted source
        // layout, fresh source traversal, then layout of the returned overlay.
        hover.anchors.begin(Vec::new());
        let source_node = hover.layout(
            tree,
            renderer,
            &cosmic::iced::advanced::layout::Limits::new(Size::ZERO, size),
        );
        let viewport = Rectangle::with_size(size);
        drop(hover.overlay(
            tree,
            Layout::new(&source_node),
            renderer,
            &viewport,
            Vector::ZERO,
        ));
        if !hover.anchors.claim_hover(&hover.source) {
            return None;
        }
        // Complete the same shared delay as the native pointer/keyboard path.
        hover
            .anchors
            .advance(std::time::Instant::now() + crate::popup::INFORMATION_DELAY);
        let mut overlay = hover
            .overlay(
                tree,
                Layout::new(&source_node),
                renderer,
                &viewport,
                Vector::ZERO,
            )
            .expect("claimed hover overlay");
        Some(overlay.as_overlay_mut().layout(renderer, size))
    }

    fn native_texts(
        hover: &mut Hover<'_, ()>,
        tree: &mut Tree,
        renderer: &Renderer,
        node: &cosmic::iced::advanced::layout::Node,
    ) -> Vec<String> {
        #[derive(Default)]
        struct Texts(Vec<String>);
        impl Operation for Texts {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                operate(self);
            }
            fn text(&mut self, _: Option<&Id>, _: Rectangle, text: &str) {
                self.0.push(text.to_owned());
            }
        }
        let mut texts = Texts::default();
        hover.body.as_widget_mut().operate(
            &mut tree.children[1],
            Layout::new(&node.children()[0]),
            renderer,
            &mut texts,
        );
        texts.0
    }

    #[test]
    fn deferred_contents_follow_accepted_resize_revision_and_source_disappearance() {
        let renderer = renderer();
        let anchors = Anchors::default();
        let source = Id::new("deferred-source");
        let inputs = Inputs::default();
        let first = Snapshot {
            revision: 1,
            expected_revision: 1,
            full: "Full one",
            extra: "Extra one",
        };
        let mut hover = hover(&anchors, source.clone(), first, inputs.clone());
        let mut tree = Tree::new(&hover as &dyn Widget<(), Theme, Renderer>);
        let compact =
            accepted_overlay(&mut hover, &mut tree, &renderer, Size::new(200.0, 200.0)).unwrap();
        assert_eq!(compact.children().len(), 1);
        assert_eq!(
            native_texts(&mut hover, &mut tree, &renderer, &compact),
            ["Full one", "Extra one"]
        );
        let compact_report = inputs.borrow().last().unwrap().clone().unwrap();
        assert_eq!(compact_report.level, Level::Mild);
        assert_eq!(compact_report.revision, 1);
        assert_eq!(compact_report.losses.len(), 1);
        let expanded =
            accepted_overlay(&mut hover, &mut tree, &renderer, Size::new(400.0, 200.0)).unwrap();
        assert_eq!(
            native_texts(&mut hover, &mut tree, &renderer, &expanded),
            ["Extra one"]
        );
        assert!(
            inputs
                .borrow()
                .last()
                .unwrap()
                .as_ref()
                .unwrap()
                .losses
                .is_empty()
        );
        let second = Snapshot {
            revision: 2,
            expected_revision: 2,
            full: "Full two",
            extra: "Extra two",
        };
        hover = self::hover(&anchors, source.clone(), second, inputs.clone());
        hover.diff(&mut tree);
        let updated =
            accepted_overlay(&mut hover, &mut tree, &renderer, Size::new(200.0, 200.0)).unwrap();
        assert_eq!(
            native_texts(&mut hover, &mut tree, &renderer, &updated),
            ["Full two", "Extra two"]
        );
        assert_eq!(
            inputs.borrow().last().unwrap().as_ref().unwrap().revision,
            2
        );
        let calls = inputs.borrow().len();
        hover.content = widget::space().width(0).height(0).into();
        hover.diff(&mut tree);
        assert!(
            accepted_overlay(&mut hover, &mut tree, &renderer, Size::new(200.0, 200.0)).is_none()
        );
        assert_eq!(inputs.borrow().len(), calls);
        assert!(anchors.visible(&source).is_none());
        assert!(anchors.information(&source).is_none());
        assert!(anchors.hovered().is_none());
    }

    #[test]
    fn absent_matching_report_is_explicit_and_pure_source_queries_do_not_prepare() {
        let renderer = renderer();
        let anchors = Anchors::default();
        let source = Id::new("deferred-unknown-source");
        let inputs = Inputs::default();
        let snapshot = Snapshot {
            revision: 1,
            expected_revision: 2,
            full: "Full one",
            extra: "Extra one",
        };
        let queried = anchors.clone();
        let queried_source = source.clone();
        let observed = inputs.clone();
        let probe: layout::Child<'_, ()> = layout::child(move |_| {
            hover_box_with_report(
                queried.clone(),
                queried_source.clone(),
                source_target(&queried, queried_source.clone(), snapshot),
                prepare(snapshot, observed.clone()),
                vec![AnchorCandidate::new(Side::Below, Alignment::StartBounded)],
            )
        });
        probe
            .query_at(Level::Preferred, &renderer, Size::new(400.0, 200.0))
            .unwrap();
        assert!(inputs.borrow().is_empty());
        assert!(anchors.information(&source).is_none());
        let mut hover = hover(&anchors, source, snapshot, inputs.clone());
        let mut tree = Tree::new(&hover as &dyn Widget<(), Theme, Renderer>);
        let node =
            accepted_overlay(&mut hover, &mut tree, &renderer, Size::new(200.0, 200.0)).unwrap();
        assert_eq!(inputs.borrow().len(), 1);
        assert!(inputs.borrow()[0].is_none());
        assert_eq!(
            native_texts(&mut hover, &mut tree, &renderer, &node),
            ["Unknown report", "Extra one"]
        );
    }
}
