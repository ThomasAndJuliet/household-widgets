// SPDX-License-Identifier: GPL-3.0-only

//! Live source geometry is collected during native overlay traversal. Pure
//! staged-engine measurements only invoke layout, so disposable probes never
//! publish geometry into this registry.

use cosmic::iced::advanced::widget::{Id, Operation, Tree};
use cosmic::iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use cosmic::iced::{Event, Length, Rectangle, Size, Vector, keyboard};
use cosmic::{Element, Renderer, Theme};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Pointer and keyboard targeting share this discovery delay.
pub const INFORMATION_DELAY: Duration = Duration::from_millis(500);

#[derive(Clone, Default)]
pub struct Anchors(Rc<RefCell<Registry>>);

#[derive(Clone)]
struct Registry {
    visible: HashMap<SourceKey, Source>,
    present: HashSet<SourceKey>,
    information: HashMap<SourceKey, crate::layout::SourceReport>,
    open: HashSet<SourceKey>,
    surfaces: Vec<Rectangle>,
    hovered: Option<Id>,
    pointer_owner: bool,
    deadline: Option<Instant>,
    opened: bool,
    suppressed: bool,
    active: bool,
    help: HashSet<SourceKey>,
    focus_sources: HashMap<SourceKey, Id>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            visible: HashMap::new(),
            present: HashSet::new(),
            information: HashMap::new(),
            open: HashSet::new(),
            surfaces: Vec::new(),
            hovered: None,
            pointer_owner: false,
            deadline: None,
            opened: false,
            suppressed: false,
            active: true,
            help: HashSet::new(),
            focus_sources: HashMap::new(),
        }
    }
}

// The pinned native Custom ID compares by name but hashes its allocation
// number. Keep that upstream inconsistency out of registry hash collections;
// focus operations and widget events still receive the original native IDs.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum SourceKey {
    Named(Cow<'static, str>),
    Unique(u64),
    Set(Vec<SourceKey>),
}
impl SourceKey {
    fn of(id: &Id) -> Self {
        Self::internal(&id.0)
    }
    fn internal(id: &cosmic::iced::core::id::Internal) -> Self {
        use cosmic::iced::core::id::Internal;
        match id {
            Internal::Custom(_, name) => Self::Named(name.clone()),
            Internal::Unique(number) => Self::Unique(*number),
            Internal::Set(ids) => Self::Set(ids.iter().map(Self::internal).collect()),
        }
    }
}

#[derive(Clone, Copy)]
struct Source {
    bounds: Rectangle,
    trigger: bool,
}

impl Anchors {
    /// Native trial traversals discover clipped descendant sources without
    /// publishing rejected geometry, source reports, or hover ownership.
    pub(crate) fn snapshot(&self) -> RegistrySnapshot {
        RegistrySnapshot(self.0.borrow().clone())
    }
    pub(crate) fn restore(&self, snapshot: &RegistrySnapshot) {
        *self.0.borrow_mut() = snapshot.0.clone();
    }
    pub(crate) fn transaction(&self, base: &RegistrySnapshot) -> RegistryTransaction {
        let saved = self.0.borrow().clone();
        self.restore(base);
        RegistryTransaction {
            anchors: self.clone(),
            saved: Some(saved),
        }
    }
    /// Register arbitrary content as a source for menus or informational hover.
    pub fn anchor<'a, Message: 'a>(
        &self,
        id: impl Into<Id>,
        content: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        self.wrap(id.into(), content.into(), false)
    }
    /// A menu-opening source. Outside presses over other visible triggers may
    /// reach their native children, allowing a single press to switch menus.
    /// Give a focusable trigger child the same native ID for focus restoration.
    pub fn trigger<'a, Message: 'a>(
        &self,
        id: impl Into<Id>,
        content: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        self.wrap(id.into(), content.into(), true)
    }
    fn wrap<'a, Message: 'a>(
        &self,
        id: Id,
        content: Element<'a, Message>,
        trigger: bool,
    ) -> Element<'a, Message> {
        Element::new(SourceWidget {
            id,
            content,
            anchors: self.clone(),
            trigger,
            information: None,
        })
    }
    /// Register a source and collect its current, verified layout-box report.
    /// The revision must identify the prepared content used by the caller.
    /// Wrap this in `trigger` too when it opens a menu.
    pub fn reported_anchor<'a, Message: 'a>(
        &self,
        id: impl Into<Id>,
        source: crate::layout::SourceId,
        revision: u64,
        content: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        Element::new(SourceWidget {
            id: id.into(),
            content: content.into(),
            anchors: self.clone(),
            trigger: false,
            information: Some((source, revision)),
        })
    }
    /// Information loss reported by the currently accepted source layout.
    /// Absence is not a claim that all information is visible.
    pub fn information(&self, id: &Id) -> Option<crate::layout::SourceReport> {
        self.0.borrow().information.get(&SourceKey::of(id)).cloned()
    }
    /// The source exists in the native traversal, even when clipped out of view.
    pub(crate) fn present(&self, id: &Id) -> bool {
        self.0.borrow().present.contains(&SourceKey::of(id))
    }
    /// The positive-area intersection of the source and its native clip viewport.
    pub fn visible(&self, id: &Id) -> Option<Rectangle> {
        self.0
            .borrow()
            .visible
            .get(&SourceKey::of(id))
            .map(|source| source.bounds)
    }
    /// State comes from the actual host chain, rather than per-tooltip booleans.
    pub fn is_open(&self, id: &Id) -> bool {
        self.0.borrow().open.contains(&SourceKey::of(id))
    }
    /// Existing committed panel rectangles, in paint order, for hover avoidance.
    pub fn surfaces(&self) -> Vec<Rectangle> {
        self.0.borrow().surfaces.clone()
    }
    pub fn hovered(&self) -> Option<Id> {
        let registry = self.0.borrow();
        registry
            .hovered
            .as_ref()
            .filter(|id| {
                let key = SourceKey::of(id);
                registry.visible.contains_key(&key) && !registry.open.contains(&key)
            })
            .cloned()
    }
    fn target_at(&self, id: &Id, pointer: bool, now: Instant) -> bool {
        let mut registry = self.0.borrow_mut();
        let source = registry
            .focus_sources
            .get(&SourceKey::of(id))
            .cloned()
            .unwrap_or_else(|| id.clone());
        let key = SourceKey::of(&source);
        if !registry.active
            || registry.open.contains(&key)
            || (pointer && !registry.visible.contains_key(&key))
            || (!pointer && !registry.present.contains(&key))
            || !registry.help.contains(&key)
        {
            if registry.hovered.as_ref() == Some(&source) {
                Self::clear_target(&mut registry);
            }
            return false;
        }
        if registry.hovered.as_ref() != Some(&source) || registry.pointer_owner != pointer {
            registry.hovered = Some(source);
            registry.pointer_owner = pointer;
            registry.deadline = Some(now + INFORMATION_DELAY);
            registry.opened = false;
            registry.suppressed = false;
        }
        true
    }
    /// A deliberate navigation action targets information without moving or
    /// activating the control. Ordinary typing must not call this operation.
    pub fn keyboard_target(&self, id: &Id) {
        if !self.target_at(id, false, Instant::now()) {
            Self::clear_target(&mut self.0.borrow_mut());
        }
    }
    pub(crate) fn claim_hover(&self, id: &Id) -> bool {
        self.target_at(id, true, Instant::now())
    }
    fn clear_target(registry: &mut Registry) {
        registry.hovered = None;
        registry.deadline = None;
        registry.opened = false;
        registry.suppressed = false;
    }
    pub(crate) fn release_hover(&self, id: &Id) {
        let mut registry = self.0.borrow_mut();
        if registry.pointer_owner && registry.hovered.as_ref() == Some(id) {
            Self::clear_target(&mut registry);
        }
    }
    pub(crate) fn leave_keyboard_target(&self) {
        let mut registry = self.0.borrow_mut();
        if !registry.pointer_owner {
            Self::clear_target(&mut registry);
        }
    }
    /// Run after the accepted native traversal, never during a trial query.
    pub(crate) fn prune_information(&self) {
        let mut registry = self.0.borrow_mut();
        let invalid = registry.hovered.as_ref().is_some_and(|id| {
            let key = SourceKey::of(id);
            !registry.visible.contains_key(&key)
                || !registry.help.contains(&key)
                || registry.open.contains(&key)
        });
        if invalid {
            Self::clear_target(&mut registry);
        }
    }
    pub(crate) fn hover_is_owner(&self, id: &Id) -> bool {
        self.hovered().as_ref() == Some(id)
    }
    pub(crate) fn information_visible(&self, id: &Id) -> bool {
        self.hover_is_owner(id) && self.0.borrow().opened
    }
    pub(crate) fn register_information(&self, source: &Id, focus: impl IntoIterator<Item = Id>) {
        let mut registry = self.0.borrow_mut();
        registry.help.insert(SourceKey::of(source));
        for id in focus {
            registry
                .focus_sources
                .insert(SourceKey::of(&id), source.clone());
        }
    }
    /// Suppress automatic reopening for this unchanged target. Alt+H or a new
    /// deliberate targeting action can reopen it. Returns whether a card was open.
    pub fn dismiss_information(&self) -> bool {
        let mut registry = self.0.borrow_mut();
        let opened = registry.opened;
        registry.opened = false;
        registry.deadline = None;
        registry.suppressed = true;
        opened
    }
    /// Cancel ownership and every pending opening when the native window leaves
    /// focus. A later activation starts with no information target.
    pub fn deactivate(&self) {
        let mut registry = self.0.borrow_mut();
        registry.active = false;
        Self::clear_target(&mut registry);
    }
    pub(crate) fn activate(&self) {
        self.0.borrow_mut().active = true;
    }
    pub(crate) fn advance(&self, now: Instant) -> (bool, Option<Instant>) {
        let mut registry = self.0.borrow_mut();
        if !registry.active {
            return (false, None);
        }
        if registry.deadline.is_some_and(|at| now >= at) {
            registry.deadline = None;
            registry.opened = registry.hovered.as_ref().is_some_and(|id| {
                let key = SourceKey::of(id);
                registry.visible.contains_key(&key) && !registry.open.contains(&key)
            });
            return (registry.opened, None);
        }
        (false, registry.deadline)
    }
    pub(crate) fn update_timer<Message>(&self, event: &Event, shell: &mut Shell<'_, Message>) {
        let now = match event {
            Event::Window(cosmic::iced::window::Event::RedrawRequested(now)) => *now,
            _ => Instant::now(),
        };
        let (changed, deadline) = self.advance(now);
        if changed {
            shell.invalidate_layout();
            shell.request_redraw();
        }
        if let Some(at) = deadline {
            shell.request_redraw_at(at);
        }
    }
    pub(crate) fn information_key<Message>(
        &self,
        event: &Event,
        focused: Option<&Id>,
        shell: &mut Shell<'_, Message>,
    ) -> bool {
        let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
            return false;
        };
        let information = modifiers.alt()
            && !modifiers.control()
            && !modifiers.logo()
            && matches!(key, keyboard::Key::Character(value) if value.eq_ignore_ascii_case("h"));
        if information {
            if let Some(id) = focused {
                let source = {
                    let registry = self.0.borrow();
                    registry
                        .focus_sources
                        .get(&SourceKey::of(id))
                        .cloned()
                        .or_else(|| {
                            registry
                                .help
                                .contains(&SourceKey::of(id))
                                .then(|| id.clone())
                        })
                };
                if let Some(source) = source {
                    self.target_at(&source, false, Instant::now());
                    let mut registry = self.0.borrow_mut();
                    if registry.hovered.as_ref() == Some(&source) && registry.active {
                        registry.deadline = None;
                        registry.opened = true;
                        registry.suppressed = false;
                    }
                }
            }
            shell.invalidate_layout();
            shell.request_redraw();
            shell.capture_event();
            return true;
        }
        if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape))
            && self.dismiss_information()
        {
            shell.invalidate_layout();
            shell.request_redraw();
            shell.capture_event();
            return true;
        }
        false
    }
    pub(crate) fn begin(&self, open: impl IntoIterator<Item = Id>) {
        let mut registry = self.0.borrow_mut();
        registry.visible.clear();
        registry.present.clear();
        registry.information.clear();
        registry.help.clear();
        registry.focus_sources.clear();
        registry.open = open.into_iter().map(|id| SourceKey::of(&id)).collect();
    }
    pub(crate) fn set_surfaces(&self, surfaces: Vec<Rectangle>) {
        self.0.borrow_mut().surfaces = surfaces;
    }
    pub(crate) fn focused_trigger(&self, id: &Id) -> bool {
        let registry = self.0.borrow();
        let source = registry.focus_sources.get(&SourceKey::of(id)).unwrap_or(id);
        registry
            .visible
            .get(&SourceKey::of(source))
            .is_some_and(|source| source.trigger)
    }
    pub(crate) fn trigger_at(&self, point: cosmic::iced::Point, excluded: Option<&Id>) -> bool {
        let registry = self.0.borrow();
        let excluded = excluded.map(SourceKey::of);
        // Closing the already-open root consumes its press. Otherwise its
        // native pressed state could survive the close rebuild and reopen the
        // menu on release using the new view's opening callback.
        if excluded
            .as_ref()
            .and_then(|id| registry.visible.get(id))
            .is_some_and(|source| source.bounds.contains(point))
        {
            return false;
        }
        registry.visible.iter().any(|(id, source)| {
            Some(id) != excluded.as_ref() && source.trigger && source.bounds.contains(point)
        })
    }
    fn record(&self, id: Id, bounds: Rectangle, viewport: Rectangle, trigger: bool) {
        self.0.borrow_mut().present.insert(SourceKey::of(&id));
        if let Some(bounds) = bounds
            .intersection(&viewport)
            .filter(|bounds| bounds.width > 0.0 && bounds.height > 0.0)
        {
            let mut registry = self.0.borrow_mut();
            let key = SourceKey::of(&id);
            let trigger = trigger
                || registry
                    .visible
                    .get(&key)
                    .is_some_and(|source| source.trigger);
            registry.visible.insert(key, Source { bounds, trigger });
        }
    }
}

#[derive(Clone)]
pub(crate) struct RegistrySnapshot(Registry);

pub(crate) struct RegistryTransaction {
    anchors: Anchors,
    saved: Option<Registry>,
}
impl Drop for RegistryTransaction {
    fn drop(&mut self) {
        if let Some(saved) = self.saved.take() {
            *self.anchors.0.borrow_mut() = saved;
        }
    }
}

struct SourceWidget<'a, Message> {
    id: Id,
    content: Element<'a, Message>,
    anchors: Anchors,
    trigger: bool,
    information: Option<(crate::layout::SourceId, u64)>,
}
impl<'a, Message: 'a> Widget<Message, Theme, Renderer> for SourceWidget<'a, Message> {
    forward_widget!(Message, Theme, Renderer; children, diff, size, layout, draw, update, operate, mouse_interaction, drag_destinations);
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let bounds = Rectangle {
            x: layout.bounds().x + translation.x,
            y: layout.bounds().y + translation.y,
            ..layout.bounds()
        };
        self.anchors
            .record(self.id.clone(), bounds, *viewport, self.trigger);
        if let Some((source, revision)) = self.information {
            let mut reports = crate::layout::SourceReports::default();
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                &mut reports,
            );
            let mut matching = reports.into_reports().into_iter().filter(|accepted| {
                accepted.report.source == source && accepted.report.revision == revision
            });
            let report = matching.next();
            let unique = matching.next().is_none();
            let key = SourceKey::of(&self.id);
            let mut registry = self.anchors.0.borrow_mut();
            registry.information.remove(&key);
            if unique
                && registry.visible.contains_key(&key)
                && let Some(report) = report
            {
                registry.information.insert(key, report.report);
            }
        }
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
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

#[cfg(test)]
mod registry_tests {
    use super::*;
    use cosmic::iced::{Point, Size};

    #[test]
    fn rebuilt_named_ids_resolve_the_same_source_without_aliasing_other_ids() {
        let anchors = Anchors::default();
        let bounds = Rectangle::new(Point::new(20.0, 30.0), Size::new(40.0, 20.0));
        let window = Rectangle::with_size(Size::new(300.0, 200.0));
        anchors.record(Id::new("source"), bounds, window, true);
        assert_eq!(
            anchors.visible(&Id::new("source")),
            Some(bounds),
            "fresh native IDs with the same custom name resolve after view rebuilding"
        );
        assert_eq!(anchors.visible(&Id::new("different-source")), None);
        // Source/hover wrappers may register the same name independently.
        anchors.record(Id::new("source"), bounds, window, false);
        assert!(
            anchors.trigger_at(Point::new(30.0, 40.0), None),
            "duplicate named source registration preserves its trigger marker"
        );
        let first = Id::unique();
        let second = Id::unique();
        anchors.record(first.clone(), bounds, window, false);
        assert_eq!(anchors.visible(&first), Some(bounds));
        assert_eq!(
            anchors.visible(&second),
            None,
            "distinct unique IDs remain distinct"
        );
    }

    #[test]
    fn named_open_and_hover_ownership_use_the_same_canonical_source_key() {
        let anchors = Anchors::default();
        let bounds = Rectangle::with_size(Size::new(40.0, 20.0));
        anchors.record(Id::new("source"), bounds, bounds, true);
        anchors.register_information(&Id::new("source"), []);
        assert!(anchors.claim_hover(&Id::new("source")));
        assert!(anchors.hover_is_owner(&Id::new("source")));
        anchors.release_hover(&Id::new("other"));
        assert!(anchors.hover_is_owner(&Id::new("source")));
        anchors.begin([Id::new("source")]);
        anchors.record(Id::new("source"), bounds, bounds, true);
        assert!(anchors.is_open(&Id::new("source")));
        assert!(!anchors.is_open(&Id::new("other")));
        assert!(
            !anchors.claim_hover(&Id::new("source")),
            "the live open source suppresses only its own hover card"
        );
        assert_eq!(anchors.hovered(), None);
    }
}

#[cfg(test)]
mod information_state_tests {
    use super::*;
    use cosmic::iced::Point;

    fn sources() -> (Anchors, Id, Id) {
        let anchors = Anchors::default();
        let first = Id::new("first-info");
        let second = Id::new("second-info");
        let window = Rectangle::with_size(Size::new(400.0, 300.0));
        for (id, x) in [(&first, 0.0), (&second, 100.0)] {
            anchors.record(
                id.clone(),
                Rectangle::new(Point::new(x, 0.0), Size::new(80.0, 40.0)),
                window,
                false,
            );
            anchors.register_information(id, []);
        }
        (anchors, first, second)
    }

    #[test]
    fn pointer_and_keyboard_share_delay_and_target_change_restarts_it() {
        let (anchors, first, second) = sources();
        let start = Instant::now();
        assert!(anchors.target_at(&first, true, start));
        assert!(!anchors.advance(start + INFORMATION_DELAY / 2).0);
        assert!(anchors.target_at(&second, false, start + INFORMATION_DELAY / 2));
        assert!(!anchors.advance(start + INFORMATION_DELAY).0);
        assert!(!anchors.information_visible(&first));
        assert!(
            anchors
                .advance(start + INFORMATION_DELAY + INFORMATION_DELAY / 2)
                .0
        );
        assert!(anchors.information_visible(&second));
    }

    #[test]
    fn dismissed_target_stays_suppressed_until_leave_and_new_target() {
        let (anchors, first, _) = sources();
        let start = Instant::now();
        anchors.target_at(&first, true, start);
        anchors.advance(start + INFORMATION_DELAY);
        assert!(anchors.dismiss_information());
        // Motion inside the same source is not entering another target.
        anchors.target_at(&first, true, start + INFORMATION_DELAY * 2);
        assert!(!anchors.advance(start + INFORMATION_DELAY * 3).0);
        assert!(!anchors.information_visible(&first));
        anchors.release_hover(&first);
        anchors.target_at(&first, true, start + INFORMATION_DELAY * 4);
        assert!(anchors.advance(start + INFORMATION_DELAY * 5).0);
    }

    #[test]
    fn latest_owner_leaving_does_not_resurrect_earlier_keyboard_target() {
        let (anchors, first, second) = sources();
        let start = Instant::now();
        anchors.target_at(&first, false, start);
        anchors.target_at(&second, true, start + INFORMATION_DELAY);
        anchors.release_hover(&second);
        assert_eq!(anchors.hovered(), None);
        assert_eq!(
            anchors.advance(start + INFORMATION_DELAY * 3),
            (false, None)
        );
        // Leaving a nonowner pointer source cannot dismiss the keyboard owner.
        anchors.target_at(&first, false, start + INFORMATION_DELAY * 4);
        anchors.release_hover(&first);
        assert_eq!(anchors.hovered(), Some(first));
    }

    #[test]
    fn deactivation_and_disappearing_source_cancel_pending_openings() {
        let (anchors, first, _) = sources();
        let start = Instant::now();
        anchors.target_at(&first, false, start);
        anchors.deactivate();
        anchors.activate();
        assert_eq!(
            anchors.advance(start + INFORMATION_DELAY * 3),
            (false, None)
        );
        assert_eq!(anchors.hovered(), None);
        anchors.target_at(&first, false, start + INFORMATION_DELAY * 4);
        anchors.begin([]);
        anchors.prune_information();
        assert_eq!(
            anchors.advance(start + INFORMATION_DELAY * 6),
            (false, None)
        );
    }

    #[test]
    fn rejected_native_probe_cannot_publish_information_ownership_or_timer() {
        let (anchors, first, second) = sources();
        let start = Instant::now();
        anchors.target_at(&first, false, start);
        let snapshot = anchors.snapshot();
        {
            let _trial = anchors.transaction(&snapshot);
            anchors.target_at(&second, true, start + INFORMATION_DELAY);
            anchors.advance(start + INFORMATION_DELAY * 3);
            anchors.deactivate();
        }
        assert_eq!(anchors.hovered(), Some(first.clone()));
        assert!(anchors.advance(start + INFORMATION_DELAY).0);
        assert!(anchors.information_visible(&first));
    }
}
