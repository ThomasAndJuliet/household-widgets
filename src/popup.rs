// SPDX-License-Identifier: GPL-3.0-only

//! Native compositional popup prototype. The application owns one menu chain;
//! the host retains ordinary Iced child trees and renders it inside its window.
//! Anchors come from registered live sources or explicit geometry. Source
//! clipping, trigger switching and native focus restoration belong to the host;
//! content and navigation targets remain independent of widget kinds.

use crate::layout::{
    AttachedPlan, Attachment, ChainCard, ChainFailure, NoFit, RetainedGeometry, Stage,
    choose_attached_avoiding, choose_attached_chain, commit_attached_retaining_avoiding,
    isolate_named_state,
};
use crate::list_navigation::{Axis, Direction, EndPolicy, Entry, Navigation};
use cosmic::iced::advanced::widget::{Id, Operation, Tree, operation, tree};
use cosmic::iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use cosmic::iced::{Event, Length, Padding, Rectangle, Size, Vector, keyboard};
use cosmic::{Element, Renderer, Theme, widget};

#[path = "popup_anchor.rs"]
mod anchor;
pub use anchor::{Anchors, INFORMATION_DELAY};

#[path = "popup_focus.rs"]
mod focus;
pub use focus::{Focusable, focus_target, show_keyboard_target, state_scope};

/// An explicitly participating menu navigation target. Native children always
/// receive input first. Use only IDs of focusable children, in navigation order;
/// disabled controls remain reachable with no activation message. No inspection of widget type occurs.
pub struct FocusTarget<Message> {
    pub id: Id,
    pub activate: Option<Message>,
    pub open: Option<Message>,
}
impl<Message> FocusTarget<Message> {
    pub fn new(id: impl Into<Id>) -> Self {
        Self {
            id: id.into(),
            activate: None,
            open: None,
        }
    }
    pub fn activate(mut self, message: Message) -> Self {
        self.activate = Some(message);
        self
    }
    pub fn open(mut self, message: Message) -> Self {
        self.open = Some(message);
        self
    }
}

pub struct Panel<'a, Message> {
    pub attachment: Attachment,
    pub stages: Vec<Stage<'a, Message>>,
    pub targets: Vec<FocusTarget<Message>>,
    embedded: Vec<Vec<Stage<'a, Message>>>,
    parent_edges: bool,
    surface_insets: Padding,
    source: Option<Id>,
    focus_first: bool,
    relative: bool,
    key: u64,
    reset_native: bool,
    retained: RetainedGeometry,
    content: Element<'a, Message>,
    selection: Result<AttachedPlan, NoFit>,
}
impl<'a, Message: 'a> Panel<'a, Message> {
    pub fn new(attachment: Attachment, stages: Vec<Stage<'a, Message>>) -> Self {
        Self {
            attachment,
            stages,
            targets: Vec::new(),
            embedded: Vec::new(),
            parent_edges: false,
            surface_insets: Padding::ZERO,
            source: None,
            focus_first: false,
            relative: false,
            key: 0,
            reset_native: true,
            retained: RetainedGeometry::default(),
            content: widget::space().width(0).height(0).into(),
            selection: Err(NoFit { attempted: 0 }),
        }
    }
    /// Component-owned compositions embedding one, two, ... following logical
    /// levels in this surface. These are queried only after the next attached
    /// level exhausts its permitted candidates. The host never inspects them.
    pub fn embedded_compositions(mut self, alternatives: Vec<Vec<Stage<'a, Message>>>) -> Self {
        self.embedded = alternatives;
        self
    }
    /// Keep a row's vertical anchor while placing side candidates beyond its
    /// parent's complete surface. This includes the parent's declared chrome.
    pub fn beside_previous_surface(mut self) -> Self {
        self.parent_edges = true;
        self
    }
    /// The measured outer box includes reserved shadow. Attach visible surfaces
    /// at their actual edges while still constraining the complete paint box.
    pub fn surface_insets(mut self, insets: Padding) -> Self {
        self.surface_insets = insets;
        self
    }
    fn stages_for(&self, span: usize) -> &[Stage<'a, Message>] {
        if span == 1 {
            &self.stages
        } else {
            &self.embedded[span - 2]
        }
    }
    /// The attachment's anchor is expressed in the preceding panel's local
    /// coordinates. A submenu follows that panel when the window is resized.
    pub fn relative_to_previous(mut self) -> Self {
        self.relative = true;
        self
    }
    /// Resolve this panel's anchor from the source's live, clipped native
    /// geometry. An absent/hidden source is explicit NoFit, never stale bounds.
    pub fn source(mut self, id: impl Into<Id>) -> Self {
        self.source = Some(id.into());
        self
    }
    /// Move focus to the first opted-in target when this panel session opens.
    pub fn focus_first(mut self) -> Self {
        self.focus_first = true;
        self
    }
    /// A stable identity for this panel session. Change the key when replacing
    /// a root/menu at the same depth; keep it across normal view rebuilds and
    /// resizes. The default key is suitable for one fixed menu definition.
    pub fn key(mut self, key: u64) -> Self {
        self.key = key;
        self
    }
    pub fn focus_targets(mut self, targets: Vec<FocusTarget<Message>>) -> Self {
        self.targets = targets;
        self
    }
    pub fn selection(&self) -> &Result<AttachedPlan, NoFit> {
        &self.selection
    }
}

pub fn host<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Host<'a, Message> {
    Host {
        content: content.into(),
        panels: Vec::new(),
        dismiss: None,
        close: None,
        no_fit: None,
        anchors: Anchors::default(),
        open_sources: Vec::new(),
    }
}

pub struct Host<'a, Message> {
    content: Element<'a, Message>,
    panels: Vec<Panel<'a, Message>>,
    dismiss: Option<Message>,
    close: Option<Box<dyn Fn(usize) -> Message + 'a>>,
    no_fit: Option<Box<dyn Fn(usize) -> Message + 'a>>,
    anchors: Anchors,
    open_sources: Vec<Id>,
}
impl<'a, Message: 'a> Host<'a, Message> {
    /// Exactly one chain per host. Opening a different root replaces this Vec.
    pub fn chain(mut self, panels: Vec<Panel<'a, Message>>) -> Self {
        self.panels = panels;
        self
    }
    pub fn anchors(mut self, anchors: Anchors) -> Self {
        self.anchors = anchors;
        self
    }
    pub fn on_dismiss(mut self, message: Message) -> Self {
        self.dismiss = Some(message);
        self
    }
    /// Escape/Left in a submenu asks the app to keep this many parent panels.
    pub fn on_close(mut self, close: impl Fn(usize) -> Message + 'a) -> Self {
        self.close = Some(Box::new(close));
        self
    }
    /// Emitted once on the next event when a panel has no complete fit. No
    /// implicit clipping or scrolling is introduced; the app chooses fallback.
    /// Actual opening sources of existing native surfaces outside this chain.
    /// They use the same own-source information suppression and timer lifecycle
    /// as in-window panels, without rebuilding or disabling their controls.
    pub fn open_sources(mut self, sources: impl IntoIterator<Item = Id>) -> Self {
        self.open_sources = sources.into_iter().collect();
        self
    }
    pub fn on_no_fit(mut self, no_fit: impl Fn(usize) -> Message + 'a) -> Self {
        self.no_fit = Some(Box::new(no_fit));
        self
    }
}

#[derive(Default)]
struct State {
    focused: Vec<Option<usize>>,
    navigation: Vec<Navigation<Id>>,
    keyboard_target: Vec<bool>,
    keyboard_entry: Option<usize>,
    entry_information: Vec<bool>,
    keys: Vec<u64>,
    reported_no_fit: Option<usize>,
    active_source: Option<Id>,
    return_focus: Option<Id>,
    capture_return_focus: bool,
    restore_pending: bool,
    forward_tab: bool,
    skip_restore: bool,
    initial_focus: Vec<bool>,
    plans: Vec<Option<AttachedPlan>>,
    spans: Vec<usize>,
    owners: Vec<usize>,
    visible_count: usize,
    sources: Vec<Option<Id>>,
    pending_focus: Option<Id>,
}

impl<'a, Message: Clone + 'a> Widget<Message, Theme, Renderer> for Host<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn children(&self) -> Vec<Tree> {
        std::iter::once(Tree::new(self.content.as_widget()))
            .chain(
                self.panels
                    .iter()
                    .map(|panel| Tree::new(panel.content.as_widget())),
            )
            .collect()
    }
    fn diff(&mut self, tree: &mut Tree) {
        if tree.children.is_empty() {
            tree.children.push(Tree::new(self.content.as_widget()));
        }
        tree.children[0].diff(self.content.as_widget_mut());
        tree.children.truncate(1 + self.panels.len());
        while tree.children.len() < 1 + self.panels.len() {
            tree.children.push(Tree::empty());
        }
        let state = tree.state.downcast_mut::<State>();
        let was_open = !state.keys.is_empty();
        let will_open = !self.panels.is_empty();
        if !was_open && will_open {
            state.capture_return_focus = true;
        }
        if was_open && !will_open {
            state.restore_pending = !state.skip_restore;
            state.skip_restore = false;
        }
        if will_open {
            state.active_source = self.panels[0].source.clone();
        }
        if self.panels.len() < state.sources.len() && !self.panels.is_empty() {
            state.pending_focus = state.sources[self.panels.len()].clone();
        }
        state.sources = self
            .panels
            .iter()
            .map(|panel| panel.source.clone())
            .collect();
        state.spans.resize(self.panels.len(), 1);
        state.owners.resize(self.panels.len(), 0);
        state.focused.resize(self.panels.len(), None);
        state.navigation.resize_with(self.panels.len(), || {
            Navigation::new(Axis::Vertical).end_policy(EndPolicy::Wrap)
        });
        let active_depth = state.focused.iter().rposition(Option::is_some);
        state.keyboard_target.resize(self.panels.len(), false);
        state.entry_information.resize(self.panels.len(), false);
        let keyboard_entry = state.keyboard_entry.take();
        for (index, panel) in self.panels.iter().enumerate() {
            let previous = state.navigation[index].current().cloned();
            state.navigation[index].reconcile(
                &panel
                    .targets
                    .iter()
                    .map(|target| {
                        Entry::new(
                            target.id.clone(),
                            target.activate.is_some() || target.open.is_some(),
                        )
                    })
                    .collect::<Vec<_>>(),
            );
            let current = state.navigation[index].current().cloned();
            if active_depth == Some(index)
                && state.keys.get(index) == Some(&panel.key)
                && previous.is_some()
                && previous != current
            {
                state.pending_focus = current;
            }
            state.focused[index] = state.navigation[index]
                .current()
                .and_then(|id| panel.targets.iter().position(|target| &target.id == id));
        }
        state.initial_focus.resize(self.panels.len(), false);
        state.plans.resize(self.panels.len(), None);
        for (i, panel) in self.panels.iter_mut().enumerate() {
            // A view rebuild may change callbacks or app-owned values without
            // changing geometry. Retention applies only between such diffs.
            panel.retained.clear();
            panel.reset_native = state.keys.get(i) != Some(&panel.key);
            if panel.reset_native {
                tree.children[i + 1] = Tree::empty();
                state.focused[i] = None;
                state.navigation[i].clear();
                state.keyboard_target[i] = keyboard_entry == Some(i);
                state.entry_information[i] = keyboard_entry == Some(i);
                state.reported_no_fit = None;
                state.initial_focus[i] = panel.focus_first;
                state.plans[i] = None;
            }
        }
        state.keys = self.panels.iter().map(|panel| panel.key).collect();
        if self.panels.is_empty() {
            state.reported_no_fit = None;
        }
    }
    forward_widget!(Message, Theme, Renderer; size, draw, operate, mouse_interaction, drag_destinations);
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
        if matches!(event, Event::Window(cosmic::iced::window::Event::Unfocused)) {
            self.anchors.deactivate();
            tree.state.downcast_mut::<State>().keyboard_entry = None;
            if !self.panels.is_empty()
                && let Some(message) = &self.dismiss
            {
                shell.publish(message.clone());
            }
        } else if matches!(event, Event::Window(cosmic::iced::window::Event::Focused)) {
            self.anchors.activate();
        }
        // Tab leaves a menu through the same native group route as the
        // surrounding controls. Restore its opener before the base wrapper
        // handles this event; the close rebuild must not override that move.
        let state = tree.state.downcast_mut::<State>();
        if state.forward_tab {
            state.forward_tab = false;
            if let Some(source) = state
                .active_source
                .clone()
                .or_else(|| state.return_focus.clone())
            {
                self.content.as_widget_mut().operate(
                    &mut tree.children[0],
                    layout,
                    renderer,
                    &mut operation::focusable::focus::<()>(source),
                );
            }
        }
        let mut before = FocusProbe::default();
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, &mut before);
        if matches!(event, Event::Keyboard(_) | Event::Mouse(_)) {
            let state = tree.state.downcast_mut::<State>();
            state.keyboard_entry = None;
            if self.panels.is_empty()
                && is_opening_event(event)
                && before
                    .focused
                    .as_ref()
                    .is_some_and(|id| self.anchors.focused_trigger(id))
            {
                state.keyboard_entry = Some(0);
            }
        }
        // Information dismissal has priority over native Escape handling.
        if matches!(
            event,
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            })
        ) && self
            .anchors
            .information_key(event, before.focused.as_ref(), shell)
        {
            return;
        }
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
        let mut after = FocusProbe::default();
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, &mut after);
        if is_navigation_event(event) && before.focused != after.focused {
            if let Some(id) = &after.focused {
                self.anchors.keyboard_target(id);
            } else {
                self.anchors.leave_keyboard_target();
            }
        }
        if !shell.is_event_captured() {
            self.anchors
                .information_key(event, after.focused.as_ref(), shell);
        }
        self.anchors.update_timer(event, shell);
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let node = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        let state = tree.state.downcast_mut::<State>();
        state.plans.resize(self.panels.len(), None);
        if state.capture_return_focus {
            state.capture_return_focus = false;
            let mut probe = FocusProbe::default();
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                Layout::new(&node),
                renderer,
                &mut probe,
            );
            state.return_focus = probe.focused;
            // The menu owns keyboard focus while open. The saved native ID is
            // restored when it closes; children still get events before keys.
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                Layout::new(&node),
                renderer,
                &mut operation::focusable::focus::<()>(Id::unique()),
            );
        }
        // UserInterface::build extracts named state before layout and clears
        // the global map afterwards. Reconcile the previously selected popup
        // shape here so its named native controls reclaim pressed/focus state
        // before that clear. Requery the current prepared component: the old
        // plan belongs to the previous immutable data/presentation snapshot.
        // Its width is only an offer for this state-reconciliation pass.
        // This geometry is never drawn, published, or used for input:
        // Chain::layout still queries the current live source and commits a
        // fresh fitting plan, including after expansion or source movement.
        for (i, panel) in self.panels.iter_mut().enumerate() {
            let span = state
                .spans
                .get(i)
                .copied()
                .unwrap_or(1)
                .min(panel.embedded.len() + 1);
            if !panel.reset_native
                && let Some(plan) = &state.plans[i]
                && let Some(stage) = panel.stages_for(span).get(plan.stage.stage_index)
                && let Ok(geometry) = stage.content.query_at(
                    stage.level,
                    renderer,
                    Size::new(plan.bounds.width, f32::INFINITY),
                )
            {
                let mut current = plan.clone();
                current.bounds.width = geometry.size.width;
                current.bounds.height = geometry.size.height;
                current.stage.level = stage.level;
                current.stage.geometry = geometry;
                let stages = if span == 1 {
                    &panel.stages
                } else {
                    &panel.embedded[span - 2]
                };
                let _ = current.commit(
                    stages,
                    renderer,
                    &mut panel.content,
                    &mut tree.children[i + 1],
                );
            }
        }
        node
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let (base, panel_trees) = tree.children.split_first_mut().expect("host base tree");
        self.anchors.begin(
            self.panels
                .iter()
                .filter_map(|panel| panel.source.clone())
                .chain(self.open_sources.iter().cloned()),
        );
        if self.panels.is_empty() {
            self.anchors.set_surfaces(Vec::new());
        }
        let state = tree.state.downcast_mut::<State>();
        if state.restore_pending {
            state.restore_pending = false;
            // One discovery traversal makes clipping current before choosing a
            // restoration target. The real overlay is obtained below.
            drop(self.content.as_widget_mut().overlay(
                base,
                layout,
                renderer,
                viewport,
                translation,
            ));
            let source = state
                .active_source
                .take()
                .filter(|id| self.anchors.visible(id).is_some());
            if let Some(target) = source.or_else(|| state.return_focus.take()) {
                self.content.as_widget_mut().operate(
                    base,
                    layout,
                    renderer,
                    &mut operation::focusable::focus::<()>(target),
                );
            }
        }
        let base_overlay =
            self.content
                .as_widget_mut()
                .overlay(base, layout, renderer, viewport, translation);
        if self.panels.is_empty() {
            self.anchors.prune_information();
            return base_overlay;
        }
        let base_registry = self.anchors.snapshot();
        let chain = overlay::Element::new(Box::new(Chain {
            panels: &mut self.panels,
            trees: panel_trees,
            state,
            dismiss: self.dismiss.as_ref(),
            close: self.close.as_deref(),
            no_fit: self.no_fit.as_deref(),
            viewport: *viewport,
            translation,
            anchors: self.anchors.clone(),
            base_registry,
        }));
        Some(match base_overlay {
            // Native Tooltip returns a full-window Group around its bubble.
            // Its index (1) places that entire group after Chain (0.5), so
            // unrelated hover explanations can paint above the menu.
            Some(base) => overlay::Group::with_children(vec![base, chain]).overlay(),
            None => chain,
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

impl<'a, Message: Clone + 'a> From<Host<'a, Message>> for Element<'a, Message> {
    fn from(host: Host<'a, Message>) -> Self {
        Element::new(host)
    }
}

struct Chain<'b, 'a, Message> {
    panels: &'b mut [Panel<'a, Message>],
    trees: &'b mut [Tree],
    state: &'b mut State,
    dismiss: Option<&'b Message>,
    close: Option<&'b (dyn Fn(usize) -> Message + 'a)>,
    no_fit: Option<&'b (dyn Fn(usize) -> Message + 'a)>,
    viewport: Rectangle,
    translation: Vector,
    anchors: Anchors,
    base_registry: anchor::RegistrySnapshot,
}

impl<Message: Clone> Chain<'_, '_, Message> {
    fn dismiss(&self, shell: &mut Shell<'_, Message>) {
        if let Some(message) = self.dismiss {
            shell.publish(message.clone());
        }
        shell.capture_event();
    }
}

impl<'a, Message: Clone + 'a> overlay::Overlay<Message, Theme, Renderer>
    for Chain<'_, 'a, Message>
{
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        self.viewport = Rectangle::with_size(bounds);
        self.state.focused.resize(self.panels.len(), None);
        self.state.keyboard_target.resize(self.panels.len(), false);
        self.state
            .entry_information
            .resize(self.panels.len(), false);
        self.state.navigation.resize_with(self.panels.len(), || {
            Navigation::new(Axis::Vertical).end_policy(EndPolicy::Wrap)
        });
        for (index, panel) in self.panels.iter().enumerate() {
            self.state.navigation[index].reconcile(
                &panel
                    .targets
                    .iter()
                    .map(|target| {
                        Entry::new(
                            target.id.clone(),
                            target.activate.is_some() || target.open.is_some(),
                        )
                    })
                    .collect::<Vec<_>>(),
            );
        }
        self.state.plans.resize(self.panels.len(), None);
        let original_owners = self.state.owners.clone();
        // Native scrolling can make a declared source unavailable for a
        // detached card even when a fresh trial starts with scroll offset zero.
        // Each rejected detached boundary is permanent for this negotiation,
        // so at most one retry per logical level is possible.
        let mut rejected_detached = vec![false; self.panels.len()];
        loop {
            let selection = choose_attached_chain(
                self.panels.len(),
                |index| self.panels[index].embedded.len() + 1,
                |prefix, index, span| {
                    if rejected_detached[index] {
                        return Err(ChainFailure::NoFit(NoFit { attempted: 0 }));
                    }
                    // All source discovery for trial ownership uses fresh trees
                    // and a reversible registry. No trial consumes named state or
                    // changes live focus, hover ownership, or accepted reports.
                    let _named = isolate_named_state();
                    let _registry = self.anchors.transaction(&self.base_registry);
                    for card in prefix {
                        let parent = &self.panels[card.index];
                        let mut content = widget::space().into();
                        let mut tree = Tree::empty();
                        let node = card
                            .plan
                            .commit(
                                parent.stages_for(card.span),
                                renderer,
                                &mut content,
                                &mut tree,
                            )
                            .map_err(ChainFailure::NoFit)?;
                        drop(content.as_widget_mut().overlay(
                            &mut tree,
                            Layout::new(&node),
                            renderer,
                            &self.viewport,
                            Vector::ZERO,
                        ));
                    }
                    let panel = &self.panels[index];
                    let attachment = resolved_attachment(
                        panel,
                        prefix.last(),
                        prefix.last().map_or(Padding::ZERO, |parent| {
                            self.panels[parent.index].surface_insets
                        }),
                        &self.anchors,
                        self.translation,
                    )
                    .ok_or_else(|| {
                        let error = NoFit { attempted: 0 };
                        if panel
                            .source
                            .as_ref()
                            .is_some_and(|source| self.anchors.present(source))
                        {
                            // A valid row can be outside its parent's explicit
                            // scrolling viewport. It still permits embedding.
                            ChainFailure::NoFit(error)
                        } else {
                            ChainFailure::MissingSource(error)
                        }
                    })?;
                    let obstacles: Vec<_> = prefix
                        .iter()
                        .take(prefix.len().saturating_sub(1))
                        .map(|card| card.plan.bounds)
                        .collect();
                    choose_attached_avoiding(
                        panel.stages_for(span),
                        renderer,
                        self.viewport,
                        &attachment,
                        &obstacles,
                    )
                    .map_err(ChainFailure::NoFit)
                },
            );
            let old_owners = self.state.owners.clone();
            let mut owners = vec![usize::MAX; self.panels.len()];
            let mut spans = vec![1; self.panels.len()];
            for card in &selection.cards {
                owners[card.index..card.index + card.span].fill(card.index);
                spans[card.index] = card.span;
            }
            // Moving named native controls between physical surfaces transfers the
            // same compatible state. Trial trees have never consumed this state.
            if owners != old_owners || spans != self.state.spans {
                for (index, tree) in self.trees.iter_mut().enumerate() {
                    if old_owners.get(index) != Some(&index)
                        || !self.state.plans.get(index).is_some_and(Option::is_some)
                    {
                        continue;
                    }
                    let named = tree.take_all_named();
                    tree::NAMED.with(|registry| registry.borrow_mut().extend(named));
                    *tree = Tree::empty();
                }
                for panel in self.panels.iter_mut() {
                    panel.retained.clear();
                }
            }
            self.state.owners = owners;
            self.state.spans = spans;
            // Embedded logical levels no longer own native content. Never replay
            // a previous detached card during the next named-state reconciliation.
            self.state.plans.fill(None);
            self.state.visible_count = selection
                .no_fit
                .as_ref()
                .map_or(self.panels.len(), |(index, _)| *index);
            let mut nodes: Vec<_> = self
                .panels
                .iter()
                .map(|_| layout::Node::new(Size::ZERO))
                .collect();
            self.anchors.restore(&self.base_registry);
            for panel in self.panels.iter_mut() {
                panel.selection = Err(NoFit { attempted: 0 });
            }
            let mut retry = false;
            let mut accepted_cards: Vec<ChainCard> = Vec::new();
            for card in &selection.cards {
                let Some(attachment) = resolved_attachment(
                    &self.panels[card.index],
                    accepted_cards.last(),
                    accepted_cards.last().map_or(Padding::ZERO, |parent| {
                        self.panels[parent.index].surface_insets
                    }),
                    &self.anchors,
                    self.translation,
                ) else {
                    self.state.visible_count = card.index;
                    if card.index > 0
                        && self.panels[card.index]
                            .source
                            .as_ref()
                            .is_some_and(|source| self.anchors.present(source))
                    {
                        rejected_detached[card.index] = true;
                        retry = true;
                    }
                    break;
                };
                let panel = &mut self.panels[card.index];
                let stages = if card.span == 1 {
                    &panel.stages
                } else {
                    &panel.embedded[card.span - 2]
                };
                let _isolation = panel.reset_native.then(isolate_named_state);
                panel.reset_native = false;
                let obstacles: Vec<_> = accepted_cards
                    .iter()
                    .take(accepted_cards.len().saturating_sub(1))
                    .map(|ancestor| ancestor.plan.bounds)
                    .collect();
                match commit_attached_retaining_avoiding(
                    stages,
                    renderer,
                    self.viewport,
                    &attachment,
                    &mut panel.content,
                    &mut self.trees[card.index],
                    &mut panel.retained,
                    &obstacles,
                ) {
                    Ok((plan, node)) => {
                        self.state.plans[card.index] = Some(plan.clone());
                        for depth in card.index..card.index + card.span {
                            self.panels[depth].selection = Ok(plan.clone());
                            self.panels[depth].reset_native = false;
                        }
                        accepted_cards.push(ChainCard {
                            index: card.index,
                            span: card.span,
                            plan,
                        });
                        let panel = &mut self.panels[card.index];
                        drop(panel.content.as_widget_mut().overlay(
                            &mut self.trees[card.index],
                            Layout::new(&node),
                            renderer,
                            &self.viewport,
                            Vector::ZERO,
                        ));
                        nodes[card.index] = node;
                    }
                    Err(_) => {
                        self.state.visible_count = card.index;
                        if card.index > 0 {
                            rejected_detached[card.index] = true;
                            retry = true;
                        }
                        break;
                    }
                }
            }
            if retry {
                continue;
            }
            // Select one logical target, then apply one native focus operation
            // across the whole accepted chain. Separate physical cards share
            // a keyboard route; focusing a child must unfocus its ancestors.
            let pending = self.state.pending_focus.take().filter(|id| {
                accepted_cards.iter().any(|card| {
                    (card.index..card.index + card.span).any(|depth| {
                        self.panels[depth]
                            .targets
                            .iter()
                            .any(|target| &target.id == id)
                    })
                })
            });
            let previous = (0..self.state.visible_count).rev().find_map(|depth| {
                self.state.focused[depth]
                    .and_then(|at| self.panels[depth].targets.get(at))
                    .map(|target| target.id.clone())
            });
            let mut initial = None;
            let mut information_entry = None;
            for card in &accepted_cards {
                for depth in card.index..card.index + card.span {
                    if self.state.initial_focus.get(depth) == Some(&true) {
                        self.state.initial_focus[depth] = false;
                        if let Some(target) = self.panels[depth].targets.first() {
                            initial = Some(target.id.clone());
                            if self.state.entry_information[depth] {
                                information_entry = Some(target.id.clone());
                            }
                        }
                        self.state.entry_information[depth] = false;
                    }
                }
            }
            let focus = initial.or(pending).or_else(|| {
                (self.state.owners != original_owners)
                    .then_some(previous)
                    .flatten()
            });
            if let Some(target) = focus {
                let depth = (0..self.state.visible_count).find(|depth| {
                    self.panels[*depth]
                        .targets
                        .iter()
                        .any(|entry| entry.id == target)
                });
                let keyboard = depth.is_some_and(|depth| self.state.keyboard_target[depth]);
                let mut native_focus = operation::focusable::focus::<()>(target.clone());
                for card in &accepted_cards {
                    let panel = &mut self.panels[card.index];
                    let layout = Layout::new(&nodes[card.index]);
                    panel.content.as_widget_mut().operate(
                        &mut self.trees[card.index],
                        layout,
                        renderer,
                        &mut native_focus,
                    );
                    panel.content.as_widget_mut().operate(
                        &mut self.trees[card.index],
                        layout,
                        renderer,
                        &mut show_keyboard_target(target.clone(), keyboard),
                    );
                }
                for depth in 0..self.state.visible_count {
                    self.state.focused[depth] = self.panels[depth]
                        .targets
                        .iter()
                        .position(|entry| entry.id == target);
                    if self.state.focused[depth].is_some() {
                        self.state.navigation[depth].set_current(&target);
                    }
                }
                if information_entry.as_ref() == Some(&target) {
                    self.anchors.keyboard_target(&target);
                }
            }
            if self.state.visible_count == self.panels.len() {
                self.state.reported_no_fit = None;
            }
            for plan in &mut self.state.plans[self.state.visible_count..] {
                *plan = None;
            }
            self.anchors.set_surfaces(
                selection
                    .cards
                    .iter()
                    .filter(|card| card.index < self.state.visible_count)
                    .map(|card| nodes[card.index].bounds())
                    .collect(),
            );
            self.anchors.prune_information();
            return layout::Node::with_children(bounds, nodes);
        }
    }
    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let top = hit_panel(layout, cursor);
        for (i, ((panel, tree), child)) in self
            .panels
            .iter()
            .zip(self.trees.iter())
            .zip(layout.children())
            .enumerate()
        {
            if child.bounds().width == 0.0 {
                continue;
            }
            panel.content.as_widget().draw(
                tree,
                renderer,
                theme,
                style,
                child,
                if top == Some(i) {
                    cursor
                } else {
                    mouse::Cursor::Unavailable
                },
                &self.viewport,
            );
        }
    }
    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        operation.traverse(&mut |operation| {
            for ((panel, tree), child) in self
                .panels
                .iter_mut()
                .zip(self.trees.iter_mut())
                .zip(layout.children())
            {
                if child.bounds().width == 0.0 {
                    continue;
                }
                panel
                    .content
                    .as_widget_mut()
                    .operate(tree, child, renderer, operation);
            }
        });
    }
    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        if matches!(event, Event::Window(cosmic::iced::window::Event::Unfocused)) {
            self.anchors.deactivate();
            self.state.keyboard_entry = None;
            self.dismiss(shell);
            return;
        }
        if matches!(event, Event::Window(cosmic::iced::window::Event::Focused)) {
            self.anchors.activate();
        }
        if matches!(
            event,
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            })
        ) && self.anchors.information_key(event, None, shell)
        {
            return;
        }
        self.anchors.update_timer(event, shell);
        let layouts: Vec<_> = layout.children().collect();
        let count = self.state.visible_count;
        if count < self.panels.len() && self.state.reported_no_fit != Some(count) {
            self.state.reported_no_fit = Some(count);
            if let Some(no_fit) = self.no_fit {
                shell.publish(no_fit(count));
            }
        }
        let top = hit_panel(layout, cursor);
        let keyboard = matches!(event, Event::Keyboard(_));
        // Native children have first refusal, including editors, sliders and
        // any future custom control. Keys only reach the deepest open panel.
        for i in (0..count).rev() {
            if self.state.owners[i] != i {
                continue;
            }
            if keyboard && self.state.owners[count - 1] != i {
                continue;
            }
            let mut before = FocusProbe::default();
            self.panels[i].content.as_widget_mut().operate(
                &mut self.trees[i],
                layouts[i],
                renderer,
                &mut before,
            );
            if matches!(event, Event::Keyboard(_) | Event::Mouse(_)) {
                self.state.keyboard_entry = None;
                if is_opening_event(event)
                    && before.focused.as_ref().is_some_and(|id| {
                        (i..(i + self.state.spans[i]).min(count)).any(|depth| {
                            self.panels[depth]
                                .targets
                                .iter()
                                .any(|target| &target.id == id && target.open.is_some())
                        })
                    })
                {
                    self.state.keyboard_entry = Some(count);
                }
            }
            self.panels[i].content.as_widget_mut().update(
                &mut self.trees[i],
                event,
                layouts[i],
                if top == Some(i) {
                    cursor
                } else {
                    mouse::Cursor::Unavailable
                },
                renderer,
                clipboard,
                shell,
                &self.viewport,
            );
            let mut probe = FocusProbe::default();
            self.panels[i].content.as_widget_mut().operate(
                &mut self.trees[i],
                layouts[i],
                renderer,
                &mut probe,
            );
            for depth in i..(i + self.state.spans[i]).min(count) {
                self.state.focused[depth] = probe.focused.as_ref().and_then(|id| {
                    self.panels[depth]
                        .targets
                        .iter()
                        .position(|target| &target.id == id)
                });
                if let Some(id) = &probe.focused
                    && self.state.navigation[depth].set_current(id)
                {
                    if matches!(
                        event,
                        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                    ) {
                        self.state.keyboard_target[depth] = false;
                    } else if is_navigation_event(event) && before.focused != probe.focused {
                        self.state.keyboard_target[depth] = true;
                    }
                }
            }
            if is_navigation_event(event) && before.focused != probe.focused {
                if let Some(id) = &probe.focused {
                    self.anchors.keyboard_target(id);
                } else {
                    self.anchors.leave_keyboard_target();
                }
            }
            if shell.is_event_captured() {
                return;
            }
            if keyboard
                && self
                    .anchors
                    .information_key(event, probe.focused.as_ref(), shell)
            {
                return;
            }
        }
        if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_))) && top.is_none() {
            if cursor.position().is_some_and(|point| {
                self.anchors.trigger_at(
                    point,
                    self.panels.first().and_then(|panel| panel.source.as_ref()),
                )
            }) {
                if let Some(message) = self.dismiss {
                    shell.publish(message.clone());
                }
            } else {
                self.dismiss(shell);
            }
            return;
        }
        let Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) = event else {
            return;
        };
        use keyboard::key::Named;
        let key = match key {
            keyboard::Key::Named(named) => *named,
            keyboard::Key::Character(value) if value.as_str() == " " => Named::Enter,
            _ => return,
        };
        if matches!(key, Named::Tab | Named::F6) {
            self.state.forward_tab = true;
            self.state.skip_restore = true;
            self.anchors.leave_keyboard_target();
            if let Some(message) = self.dismiss {
                shell.publish(message.clone());
            }
            return;
        }
        if matches!(key, Named::Escape) || (key == Named::ArrowLeft && count > 1) {
            self.anchors.leave_keyboard_target();
            if count > 1 {
                if let Some(close) = self.close {
                    shell.publish(close(count - 1));
                    shell.capture_event();
                } else {
                    self.dismiss(shell);
                }
            } else {
                self.dismiss(shell);
            }
            return;
        }
        let Some(i) = count.checked_sub(1) else {
            return;
        };
        let owner = self.state.owners[i];
        let targets = &self.panels[i].targets;
        if targets.is_empty() {
            return;
        }
        let current = self.state.navigation[i].current().cloned();
        let direction = match key {
            Named::ArrowDown => Some(Direction::Next),
            Named::ArrowUp => Some(Direction::Previous),
            Named::Home => Some(Direction::First),
            Named::End => Some(Direction::Last),
            _ => None,
        };
        if let Some(direction) = direction {
            let Some(target) = self.state.navigation[i].navigate(direction).cloned() else {
                return;
            };
            let next_index = targets.iter().position(|entry| entry.id == target);
            let mut native_focus = operation::focusable::focus::<()>(target.clone());
            for (index, child_layout) in layouts.iter().enumerate().take(count) {
                if self.state.owners[index] != index {
                    continue;
                }
                self.panels[index].content.as_widget_mut().operate(
                    &mut self.trees[index],
                    *child_layout,
                    renderer,
                    &mut native_focus,
                );
            }

            self.panels[owner].content.as_widget_mut().operate(
                &mut self.trees[owner],
                layouts[owner],
                renderer,
                &mut show_keyboard_target(target.clone(), true),
            );
            self.panels[owner].content.as_widget_mut().operate(
                &mut self.trees[owner],
                layouts[owner],
                renderer,
                &mut RevealTarget::new(target.clone()),
            );
            self.state.focused.fill(None);
            self.state.focused[i] = next_index;
            self.state.keyboard_target[i] = true;
            self.anchors.keyboard_target(&target);
            self.anchors.update_timer(event, shell);
            shell.capture_event();
            shell.invalidate_layout();
            shell.request_redraw();
        } else if let Some(target) = current
            .as_ref()
            .and_then(|id| targets.iter().find(|target| &target.id == id))
        {
            let action = match key {
                Named::Enter => target.activate.as_ref(),
                Named::ArrowRight => target.open.as_ref(),
                _ => None,
            };
            if let Some(action) = action {
                shell.publish(action.clone());
                shell.capture_event();
            }
        }
    }
    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let Some(index) = hit_panel(layout, cursor) else {
            return mouse::Interaction::None;
        };
        let child = layout.children().nth(index).expect("hit panel layout");
        self.panels[index]
            .content
            .as_widget()
            .mouse_interaction(&self.trees[index], child, cursor, &self.viewport, renderer)
            .max(mouse::Interaction::Idle)
    }
    fn overlay<'b>(
        &'b mut self,
        layout: Layout<'b>,
        renderer: &Renderer,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        // Preserve arbitrary nested child overlays, including explanations for
        // disabled rows. Base overlays are also forwarded by Host::overlay.
        let overlays: Vec<_> = self
            .panels
            .iter_mut()
            .zip(self.trees.iter_mut())
            .zip(layout.children())
            .filter_map(|((panel, tree), child)| {
                if child.bounds().width == 0.0 {
                    return None;
                }
                panel.content.as_widget_mut().overlay(
                    tree,
                    child,
                    renderer,
                    &self.viewport,
                    Vector::ZERO,
                )
            })
            .collect();
        (!overlays.is_empty()).then(|| overlay::Group::with_children(overlays).overlay())
    }
    fn index(&self) -> f32 {
        0.5
    }
}

fn resolved_attachment<Message>(
    panel: &Panel<'_, Message>,
    previous: Option<&ChainCard>,
    parent_insets: Padding,
    anchors: &Anchors,
    translation: Vector,
) -> Option<Attachment> {
    let mut attachment = panel.attachment.clone();
    if panel.parent_edges {
        attachment.surface_insets = panel.surface_insets;
    }
    if let Some(source) = &panel.source {
        attachment.anchor = anchors.visible(source)?;
        if panel.parent_edges
            && let Some(parent) = previous
        {
            let insets = parent_insets;
            attachment.anchor.x = parent.plan.bounds.x + insets.left;
            attachment.anchor.width =
                (parent.plan.bounds.width - insets.left - insets.right).max(0.0);
        }
    } else if panel.relative {
        let parent = previous?;
        attachment.anchor.x += parent.plan.bounds.x;
        attachment.anchor.y += parent.plan.bounds.y;
    } else {
        attachment.anchor.x += translation.x;
        attachment.anchor.y += translation.y;
    }
    Some(attachment)
}

#[derive(Default)]
pub(crate) struct FocusProbe {
    pub(crate) focused: Option<Id>,
    pub(crate) ids: Vec<Id>,
    focused_bounds: Option<Rectangle>,
}
impl Operation for FocusProbe {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn focusable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        state: &mut dyn operation::Focusable,
    ) {
        if let Some(id) = id {
            self.ids.push(id.clone());
        }
        if state.is_focused() {
            self.focused = id.cloned();
            self.focused_bounds = Some(bounds);
        }
    }
}

/// Native DFS operation: reveal only scroll ancestors containing the target.
/// Unrelated scrollables and nonfocused editors retain their offsets.
struct RevealTarget {
    target: Id,
    found: Option<Rectangle>,
    subtree: Option<Rectangle>,
}
impl RevealTarget {
    fn new(target: Id) -> Self {
        Self {
            target,
            found: None,
            subtree: None,
        }
    }
}
impl Operation for RevealTarget {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        let previous = self.found.take();
        operate(self);
        self.subtree = self.found;
        self.found = self.found.or(previous);
    }
    fn focusable(&mut self, id: Option<&Id>, bounds: Rectangle, _: &mut dyn operation::Focusable) {
        if id == Some(&self.target) {
            self.found = Some(bounds);
        }
    }
    fn scrollable(
        &mut self,
        _: Option<&Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        state: &mut dyn operation::Scrollable,
    ) {
        let Some(mut target) = self.subtree else {
            return;
        };
        target.x -= translation.x;
        target.y -= translation.y;
        let delta = |start: f32, extent: f32, view_start: f32, view_extent: f32| {
            if start < view_start {
                start - view_start
            } else if start + extent > view_start + view_extent {
                (start + extent - view_start - view_extent).min(start - view_start)
            } else {
                0.0
            }
        };
        let x = delta(target.x, target.width, bounds.x, bounds.width);
        let y = delta(target.y, target.height, bounds.y, bounds.height);
        state.scroll_by(
            operation::scrollable::AbsoluteOffset { x, y },
            bounds,
            content,
        );
        target.x -= x;
        target.y -= y;
        self.found = Some(target);
        self.subtree = Some(target);
    }
}

fn is_opening_event(event: &Event) -> bool {
    let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
        return false;
    };
    !modifiers.control()
        && !modifiers.alt()
        && !modifiers.logo()
        && (matches!(
            key,
            keyboard::Key::Named(keyboard::key::Named::Enter | keyboard::key::Named::ArrowRight)
        ) || matches!(key, keyboard::Key::Character(value) if value.as_str() == " "))
}

fn is_navigation_event(event: &Event) -> bool {
    matches!(
        event,
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(
                keyboard::key::Named::Tab
                    | keyboard::key::Named::ArrowUp
                    | keyboard::key::Named::ArrowDown
                    | keyboard::key::Named::ArrowLeft
                    | keyboard::key::Named::ArrowRight
                    | keyboard::key::Named::Home
                    | keyboard::key::Named::End
                    | keyboard::key::Named::F6
            ),
            ..
        })
    )
}

fn hit_panel(layout: Layout<'_>, cursor: mouse::Cursor) -> Option<usize> {
    layout
        .children()
        .enumerate()
        .filter_map(|(i, child)| {
            (child.bounds().width > 0.0 && cursor.is_over(child.bounds())).then_some(i)
        })
        .next_back()
}

#[cfg(test)]
#[path = "popup_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "popup_runtime_tests.rs"]
mod runtime_tests;

#[cfg(test)]
#[path = "popup_anchor_tests.rs"]
mod anchor_tests;

#[cfg(test)]
#[path = "popup_rebuild_tests.rs"]
mod rebuild_tests;

#[cfg(test)]
#[path = "popup_embedding_tests.rs"]
mod embedding_tests;

#[cfg(test)]
#[path = "information_runtime_tests.rs"]
mod information_runtime_tests;
