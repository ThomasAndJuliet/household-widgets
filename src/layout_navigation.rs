// SPDX-License-Identifier: GPL-3.0-only
//! Accepted section regions and history-independent viewing decisions.
use super::*;
use std::any::Any;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicU64, Ordering},
};

/// Stable identities are supplied by the containing interface.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RegionId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegionRole {
    Column,
    Viewport,
    Section,
    Header,
    Introduction,
}

/// A role belongs to one source revision. Association and reading order are
/// supplied by the content owner, not inferred from widget structure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Region {
    pub id: RegionId,
    pub role: RegionRole,
    pub revision: u64,
    pub section: Option<RegionId>,
    pub column: Option<RegionId>,
    pub order: Option<u32>,
    pub viewport: Option<RegionId>,
    pub clips: Vec<RegionId>,
}

/// Full bounds in window coordinates after scroll translation, not clipped.
#[derive(Clone, Debug, PartialEq)]
pub struct AcceptedRegion {
    pub region: Region,
    pub bounds: Rectangle,
}

/// A fresh collector per accepted-tree traversal. Trial layouts never publish.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RegionReports {
    reports: Vec<AcceptedRegion>,
}

/// Observe accepted geometry at redraw, including initial layout, reflow and
/// programmatic scrolling. Identical reports are suppressed across rebuilds.
/// This is geometry reporting, not evidence of deliberate user interaction.
/// Place around the complete reporting surface so each message is a complete
/// snapshot. The callback should preserve the source revision for the receiver.
pub fn observe_regions<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    on_change: impl Fn(RegionReports) -> Message + 'a,
) -> Element<'a, Message> {
    Element::new(RegionObserver {
        content: content.into(),
        on_change: Box::new(on_change),
    })
}

struct RegionObserver<'a, Message> {
    content: Element<'a, Message>,
    on_change: Box<dyn Fn(RegionReports) -> Message + 'a>,
}

#[derive(Default)]
struct ObserverState {
    previous: Option<RegionReports>,
}

impl<Message> Widget<Message, Theme, Renderer> for RegionObserver<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<ObserverState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(ObserverState::default())
    }

    forward_widget!(Message, Theme, Renderer; children, diff, size, size_hint, layout, draw, operate, a11y_nodes, mouse_interaction, overlay, drag_destinations);

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
        if !matches!(
            event,
            Event::Window(cosmic::iced::window::Event::RedrawRequested(_))
        ) || shell.is_layout_invalid()
            || shell.are_widgets_invalid()
        {
            return;
        }
        let mut reports = RegionReports::default();
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, &mut reports);
        let state = tree.state.downcast_mut::<ObserverState>();
        if state.previous.as_ref() != Some(&reports) {
            state.previous = Some(reports.clone());
            shell.publish((self.on_change)(reports));
        }
    }
}
impl RegionReports {
    pub fn into_reports(self) -> Vec<AcceptedRegion> {
        self.reports
    }
    pub fn reports(&self) -> &[AcceptedRegion] {
        &self.reports
    }
}
impl Operation for RegionReports {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn custom(
        &mut self,
        _: Option<&cosmic::iced::advanced::widget::Id>,
        _: Rectangle,
        state: &mut dyn Any,
    ) {
        if let Some(region) = state.downcast_ref::<AcceptedRegion>() {
            self.reports.push(region.clone());
        }
    }
}

/// Mark an existing native box. The wrapper does not alter its geometry or
/// publish during layout; only operations on the retained tree can see it.
/// When nested in a verified engine surface, its parent gates traversal on
/// whole-surface verification. Other native callers must traverse only their
/// currently accepted tree.
pub fn region_marker<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    region: Region,
) -> Element<'a, Message> {
    Element::new(RegionMarker {
        content: content.into(),
        region,
    })
}

struct RegionMarker<'a, Message> {
    content: Element<'a, Message>,
    region: Region,
}

impl<Message> Widget<Message, Theme, Renderer> for RegionMarker<'_, Message> {
    forward_widget!(Message, Theme, Renderer; children, diff, size, size_hint, layout, draw, update, mouse_interaction, overlay, drag_destinations);

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let mut bounds = layout.bounds();
        let offset = layout.virtual_offset();
        bounds.x -= offset.x;
        bounds.y -= offset.y;
        let mut report = AcceptedRegion {
            region: self.region.clone(),
            bounds,
        };
        operation.custom(None, bounds, &mut report);
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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

/// One section, in its column's accepted reading order. The owner must use
/// reports from the same accepted traversal and revision and reject duplicates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionGeometry {
    pub id: RegionId,
    pub bounds: Rectangle,
    pub introduction: Option<Rectangle>,
}

fn intersects(a: Rectangle, b: Rectangle) -> bool {
    a.width > 0.0
        && a.height > 0.0
        && b.width > 0.0
        && b.height > 0.0
        && a.x < b.x + b.width
        && b.x < a.x + a.width
        && a.y < b.y + b.height
        && b.y < a.y + a.height
}
fn contains(a: Rectangle, b: Rectangle) -> bool {
    b.width > 0.0
        && b.height > 0.0
        && b.x >= a.x
        && b.y >= a.y
        && b.x + b.width <= a.x + a.width
        && b.y + b.height <= a.y + a.height
}

/// One geometric result per column. `viewport` is the intersection of all
/// applicable clipping viewports in the same window coordinate space.
pub fn viewing_section(viewport: Rectangle, sections: &[SectionGeometry]) -> Option<RegionId> {
    let index = sections
        .iter()
        .position(|s| intersects(s.bounds, viewport))?;
    let first = sections[index];
    let midpoint = first.bounds.y + first.bounds.height / 2.0;
    if viewport.y >= midpoint
        && let Some(next) = sections.get(index + 1)
        && next
            .introduction
            .is_some_and(|intro| contains(viewport, intro))
    {
        return Some(next.id);
    }
    Some(first.id)
}

/// A fresh, validated column snapshot. Bounds are uncut; `viewport` is the
/// intersection of the explicitly declared viewport and its clip chain.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnGeometry {
    pub id: RegionId,
    pub viewport: Rectangle,
    pub sections: Vec<SectionGeometry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotError {
    MixedRevision,
    DuplicateId,
    InvalidBounds,
    MissingReference,
    InvalidAssociation,
    DuplicateOrder,
    UnsupportedClips,
}

fn valid_bounds(r: Rectangle) -> bool {
    r.x.is_finite()
        && r.y.is_finite()
        && r.width.is_finite()
        && r.height.is_finite()
        && r.width >= 0.0
        && r.height >= 0.0
        && (r.x + r.width).is_finite()
        && (r.y + r.height).is_finite()
}
fn intersection(a: Rectangle, b: Rectangle) -> Rectangle {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    Rectangle {
        x,
        y,
        width: (a.x + a.width).min(b.x + b.width).max(x) - x,
        height: (a.y + a.height).min(b.y + b.height).max(y) - y,
    }
}
fn union(a: Rectangle, b: Rectangle) -> Rectangle {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rectangle {
        x,
        y,
        width: (a.x + a.width).max(b.x + b.width) - x,
        height: (a.y + a.height).max(b.y + b.height) - y,
    }
}

impl RegionReports {
    /// Assemble only reports from one fresh native traversal and source revision.
    /// Region IDs are globally unique in that traversal. Column viewport and
    /// clip identities name Viewport reports; section/fragment clip declarations
    /// must match their column (or be omitted). Divergent nested clips are
    /// rejected until the resolver can represent them without losing visibility.
    pub fn snapshot(&self, revision: u64) -> Result<Vec<ColumnGeometry>, SnapshotError> {
        use std::collections::{HashMap, HashSet};
        let mut by_id = HashMap::new();
        for report in &self.reports {
            if report.region.revision != revision {
                return Err(SnapshotError::MixedRevision);
            }
            if !valid_bounds(report.bounds) {
                return Err(SnapshotError::InvalidBounds);
            }
            if by_id.insert(report.region.id, report).is_some() {
                return Err(SnapshotError::DuplicateId);
            }
        }
        let lookup = |id: RegionId, role: RegionRole| -> Result<&AcceptedRegion, SnapshotError> {
            by_id
                .get(&id)
                .copied()
                .filter(|r| r.region.role == role)
                .ok_or(SnapshotError::MissingReference)
        };
        let mut columns = Vec::new();
        for col in self
            .reports
            .iter()
            .filter(|r| r.region.role == RegionRole::Column)
        {
            let viewport_id = col.region.viewport.ok_or(SnapshotError::MissingReference)?;
            let mut viewport = lookup(viewport_id, RegionRole::Viewport)?.bounds;
            for clip in &col.region.clips {
                viewport = intersection(viewport, lookup(*clip, RegionRole::Viewport)?.bounds);
            }
            let mut sections = Vec::new();
            let mut orders = HashSet::new();
            for section in self.reports.iter().filter(|r| {
                r.region.role == RegionRole::Section && r.region.column == Some(col.region.id)
            }) {
                let order = section
                    .region
                    .order
                    .ok_or(SnapshotError::InvalidAssociation)?;
                if !orders.insert(order) {
                    return Err(SnapshotError::DuplicateOrder);
                }
                if section
                    .region
                    .section
                    .is_some_and(|id| id != section.region.id)
                {
                    return Err(SnapshotError::InvalidAssociation);
                }
                let mut header = None;
                let mut intro = None;
                for fragment in self.reports.iter().filter(|r| {
                    matches!(r.region.role, RegionRole::Header | RegionRole::Introduction)
                        && r.region.section == Some(section.region.id)
                }) {
                    if fragment.region.column.is_some_and(|id| id != col.region.id) {
                        return Err(SnapshotError::InvalidAssociation);
                    }
                    let target = if fragment.region.role == RegionRole::Header {
                        &mut header
                    } else {
                        &mut intro
                    };
                    *target = Some(target.map_or(fragment.bounds, |b| union(b, fragment.bounds)));
                }
                if let (Some(h), Some(i)) = (header, intro) {
                    intro = Some(union(h, i));
                }
                sections.push((
                    order,
                    SectionGeometry {
                        id: section.region.id,
                        bounds: section.bounds,
                        introduction: intro,
                    },
                ));
            }
            sections.sort_by_key(|(order, _)| *order);
            columns.push(ColumnGeometry {
                id: col.region.id,
                viewport,
                sections: sections.into_iter().map(|(_, section)| section).collect(),
            });
        }
        for report in &self.reports {
            let r = &report.region;
            if matches!(
                r.role,
                RegionRole::Section | RegionRole::Header | RegionRole::Introduction
            ) {
                let column = if r.role == RegionRole::Section {
                    lookup(
                        r.column.ok_or(SnapshotError::MissingReference)?,
                        RegionRole::Column,
                    )?
                } else {
                    let section = lookup(
                        r.section.ok_or(SnapshotError::MissingReference)?,
                        RegionRole::Section,
                    )?;
                    lookup(
                        section
                            .region
                            .column
                            .ok_or(SnapshotError::MissingReference)?,
                        RegionRole::Column,
                    )?
                };
                if r.viewport.is_some() && r.viewport != column.region.viewport {
                    return Err(SnapshotError::UnsupportedClips);
                }
                if !r.clips.is_empty() && r.clips != column.region.clips {
                    return Err(SnapshotError::UnsupportedClips);
                }
            }
        }
        Ok(columns)
    }
}

#[derive(Clone, Copy, Debug)]
enum Destination {
    Pending(RegionId),
    Visible(RegionId),
}
impl Destination {
    fn id(self) -> RegionId {
        match self {
            Self::Pending(id) | Self::Visible(id) => id,
        }
    }
}
/// Navigation associations are separate from focus and the active path.
#[derive(Default, Debug)]
pub struct NavigationState {
    generation: Arc<AtomicU64>,
    destination: Option<Destination>,
}
impl NavigationState {
    pub fn request(&mut self, destination: RegionId) -> u64 {
        let generation = self
            .generation
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        self.destination = Some(Destination::Pending(destination));
        generation
    }
    pub fn browse(&mut self) {
        self.cancel();
    }
    pub fn cancel(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.destination = None;
    }
    pub fn is_current_request(&self, generation: u64) -> bool {
        self.destination.is_some() && self.generation.load(Ordering::Relaxed) == generation
    }
    /// A queued reveal can verify that its request still owns navigation.
    pub fn token(&self, generation: u64) -> NavigationToken {
        NavigationToken {
            generation,
            current: if self.is_current_request(generation) {
                Arc::downgrade(&self.generation)
            } else {
                Weak::new()
            },
        }
    }
    pub fn retain_valid(&mut self, valid: impl FnOnce(RegionId) -> bool) {
        if self.destination.is_some_and(|d| !valid(d.id())) {
            self.cancel();
        }
    }
    /// Call exactly once for a complete, fresh accepted snapshot, before
    /// querying columns. Pending requests wait for arrival; arrived requests
    /// expire when no longer visible and cannot revive on a later reflow.
    pub fn reconcile(&mut self, columns: &[ColumnGeometry]) {
        let Some(destination) = self.destination else {
            return;
        };
        let visible = columns.iter().any(|c| {
            c.sections
                .iter()
                .any(|s| s.id == destination.id() && intersects(s.bounds, c.viewport))
        });
        match (destination, visible) {
            (Destination::Pending(id), true) => self.destination = Some(Destination::Visible(id)),
            (Destination::Visible(_), false) => self.cancel(),
            _ => {}
        }
    }
    pub fn current(&self, viewport: Rectangle, sections: &[SectionGeometry]) -> Option<RegionId> {
        if let Some(Destination::Visible(id)) = self.destination
            && sections
                .iter()
                .any(|s| s.id == id && intersects(s.bounds, viewport))
        {
            return Some(id);
        }
        viewing_section(viewport, sections)
    }
}

/// A queued request expires on replacement, cancellation or owner drop.
#[derive(Clone, Debug)]
pub struct NavigationToken {
    generation: u64,
    current: Weak<AtomicU64>,
}
impl NavigationToken {
    pub fn is_current(&self) -> bool {
        self.current
            .upgrade()
            .is_some_and(|current| current.load(Ordering::Relaxed) == self.generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rect(y: f32, height: f32) -> Rectangle {
        Rectangle {
            x: 0.0,
            y,
            width: 100.0,
            height,
        }
    }
    fn section(id: u64, y: f32, height: f32, intro: Option<Rectangle>) -> SectionGeometry {
        SectionGeometry {
            id: RegionId(id),
            bounds: rect(y, height),
            introduction: intro,
        }
    }
    #[test]
    fn reveal_token_expires_on_browsing_replacement_and_owner_drop() {
        let mut state = NavigationState::default();
        assert!(!state.token(0).is_current());
        let generation = state.request(RegionId(1));
        assert!(!state.token(generation.wrapping_add(1)).is_current());
        let browsed = state.token(generation);
        assert!(browsed.is_current());
        state.browse();
        assert!(!browsed.is_current());
        assert!(!state.token(generation.wrapping_add(1)).is_current());
        let generation = state.request(RegionId(1));
        let replaced = state.token(generation);
        let generation = state.request(RegionId(2));
        assert!(!replaced.is_current());
        let dropped = state.token(generation);
        assert!(dropped.is_current());
        drop(state);
        assert!(!dropped.is_current());
    }

    #[test]
    fn midpoint_and_full_introduction_are_both_required() {
        let sections = [
            section(1, 0.0, 100.0, None),
            section(2, 100.0, 100.0, Some(rect(100.0, 20.0))),
        ];
        assert_eq!(
            viewing_section(rect(49.0, 71.0), &sections),
            Some(RegionId(1))
        );
        assert_eq!(
            viewing_section(rect(50.0, 69.0), &sections),
            Some(RegionId(1))
        );
        assert_eq!(
            viewing_section(rect(50.0, 70.0), &sections),
            Some(RegionId(2))
        );
        assert_eq!(viewing_section(rect(200.0, 10.0), &sections), None);
    }
    #[test]
    fn destination_is_visible_only_and_browsing_cancels_even_without_motion() {
        let sections = [section(1, 0.0, 100.0, None), section(2, 100.0, 20.0, None)];
        let mut state = NavigationState::default();
        let old = state.request(RegionId(2));
        assert_eq!(
            state.current(rect(0.0, 100.0), &sections),
            Some(RegionId(1))
        );
        state.reconcile(&[ColumnGeometry {
            id: RegionId(10),
            viewport: rect(0.0, 110.0),
            sections: sections.to_vec(),
        }]);
        assert_eq!(
            state.current(rect(0.0, 110.0), &sections),
            Some(RegionId(2))
        );
        state.browse();
        assert!(!state.is_current_request(old));
        assert_eq!(
            state.current(rect(0.0, 110.0), &sections),
            Some(RegionId(1))
        );
        let next = state.request(RegionId(2));
        state.retain_valid(|id| id != RegionId(2));
        assert!(!state.is_current_request(next));
    }
    #[test]
    fn pending_waits_for_arrival_but_arrived_destination_cannot_revive() {
        let sections = vec![section(1, 0.0, 100.0, None), section(2, 100.0, 20.0, None)];
        let column = |height| ColumnGeometry {
            id: RegionId(9),
            viewport: rect(0.0, height),
            sections: sections.clone(),
        };
        let mut state = NavigationState::default();
        let request = state.request(RegionId(2));
        state.reconcile(&[column(100.0)]);
        assert!(state.is_current_request(request));
        state.reconcile(&[column(110.0)]);
        assert_eq!(
            state.current(rect(0.0, 110.0), &sections),
            Some(RegionId(2))
        );
        state.reconcile(&[column(100.0)]);
        assert!(!state.is_current_request(request));
        state.reconcile(&[column(110.0)]);
        assert_eq!(
            state.current(rect(0.0, 110.0), &sections),
            Some(RegionId(1))
        );
    }

    #[test]
    fn snapshot_validates_and_preserves_full_bounds_and_clip() {
        let mk = |id, role, bounds| AcceptedRegion {
            region: Region {
                id: RegionId(id),
                role,
                revision: 1,
                section: None,
                column: None,
                order: None,
                viewport: None,
                clips: vec![],
            },
            bounds,
        };
        let viewport = mk(1, RegionRole::Viewport, rect(0.0, 100.0));
        let clip = mk(2, RegionRole::Viewport, rect(10.0, 50.0));
        let mut col = mk(3, RegionRole::Column, rect(0.0, 120.0));
        col.region.viewport = Some(RegionId(1));
        col.region.clips = vec![RegionId(2)];
        let mut sec = mk(4, RegionRole::Section, rect(0.0, 200.0));
        sec.region.column = Some(RegionId(3));
        sec.region.order = Some(0);
        let mut header = mk(5, RegionRole::Header, rect(0.0, 10.0));
        header.region.section = Some(RegionId(4));
        let mut intro = mk(6, RegionRole::Introduction, rect(10.0, 10.0));
        intro.region.section = Some(RegionId(4));
        let mut reports = RegionReports {
            reports: vec![viewport, clip, col, sec, header, intro],
        };
        let columns = reports.snapshot(1).unwrap();
        assert_eq!(columns[0].viewport, rect(10.0, 50.0));
        assert_eq!(columns[0].sections[0].bounds, rect(0.0, 200.0));
        assert_eq!(columns[0].sections[0].introduction, Some(rect(0.0, 20.0)));
        reports.reports[5].region.clips = vec![RegionId(1)];
        assert_eq!(reports.snapshot(1), Err(SnapshotError::UnsupportedClips));
        reports.reports[5].region.clips.clear();
        reports.reports[5].region.revision = 2;
        assert_eq!(reports.snapshot(1), Err(SnapshotError::MixedRevision));
    }
}
