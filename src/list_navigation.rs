// SPDX-License-Identifier: GPL-3.0-only
//! Stable-identity navigation shared by lists, menus and control groups.
//!
//! Entries are navigation destinations, including disabled controls. Structural
//! separators are omitted. Selection is deliberately application-owned: moving
//! the current target never selects or activates it. Native children must get
//! editing keys before a host applies these navigation commands. After a move,
//! the host focuses the returned ID and scrolls its native bounds into view.

/// The arrow-key axis of one navigation group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Previous,
    Next,
    First,
    Last,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EndPolicy {
    #[default]
    Clamp,
    Wrap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry<Id> {
    pub id: Id,
    pub activatable: bool,
}

impl<Id> Entry<Id> {
    pub fn new(id: Id, activatable: bool) -> Self {
        Self { id, activatable }
    }
}

/// Navigation target and ordering, independent from application selection.
#[derive(Debug, Clone)]
pub struct Navigation<Id> {
    axis: Axis,
    entries: Vec<Entry<Id>>,
    current: Option<Id>,
    end_policy: EndPolicy,
}

impl<Id: Clone + PartialEq> Navigation<Id> {
    pub fn new(axis: Axis) -> Self {
        Self {
            axis,
            entries: Vec::new(),
            current: None,
            end_policy: EndPolicy::Clamp,
        }
    }

    pub fn end_policy(mut self, policy: EndPolicy) -> Self {
        self.end_policy = policy;
        self
    }

    pub fn axis(&self) -> Axis {
        self.axis
    }

    pub fn current(&self) -> Option<&Id> {
        self.current.as_ref()
    }

    /// Keep the same target across insertion and reordering. If it disappears,
    /// retain its former position (or the final remaining entry). A fresh list
    /// stays untargeted until its host explicitly enters it.
    pub fn reconcile(&mut self, entries: &[Entry<Id>]) {
        let previous = self
            .current
            .as_ref()
            .and_then(|id| self.entries.iter().position(|entry| &entry.id == id));
        let retained = self
            .current
            .as_ref()
            .and_then(|id| entries.iter().find(|entry| &entry.id == id))
            .map(|entry| entry.id.clone());
        self.current = retained.or_else(|| {
            previous
                .and_then(|at| entries.get(at.min(entries.len().saturating_sub(1))))
                .map(|entry| entry.id.clone())
        });
        self.entries = entries.to_vec();
    }

    pub fn set_current(&mut self, id: &Id) -> bool {
        if self.entries.iter().any(|entry| &entry.id == id) {
            self.current = Some(id.clone());
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        self.current = None;
    }

    pub fn can_activate(&self) -> bool {
        self.current.as_ref().is_some_and(|id| {
            self.entries
                .iter()
                .any(|entry| &entry.id == id && entry.activatable)
        })
    }

    /// Disabled entries remain destinations, with explicit end behavior.
    pub fn navigate(&mut self, direction: Direction) -> Option<&Id> {
        if self.entries.is_empty() {
            self.current = None;
            return None;
        }
        let current = self
            .current
            .as_ref()
            .and_then(|id| self.entries.iter().position(|entry| &entry.id == id));
        let last = self.entries.len() - 1;
        let next = match direction {
            Direction::First => 0,
            Direction::Last => last,
            Direction::Next => current.map_or(0, |at| match self.end_policy {
                EndPolicy::Clamp => (at + 1).min(last),
                EndPolicy::Wrap => (at + 1) % self.entries.len(),
            }),
            Direction::Previous => current.map_or(last, |at| match self.end_policy {
                EndPolicy::Clamp => at.saturating_sub(1),
                EndPolicy::Wrap => (at + last) % self.entries.len(),
            }),
        };
        self.current = Some(self.entries[next].id.clone());
        self.current()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_is_a_target_without_becoming_activatable() {
        let mut list = Navigation::new(Axis::Vertical).end_policy(EndPolicy::Wrap);
        list.reconcile(&[
            Entry::new("enabled", true),
            Entry::new("explanation", false),
        ]);
        assert_eq!(list.navigate(Direction::First), Some(&"enabled"));
        assert!(list.can_activate());
        assert_eq!(list.navigate(Direction::Next), Some(&"explanation"));
        assert!(!list.can_activate());
        assert_eq!(list.navigate(Direction::Next), Some(&"enabled"));
        assert_eq!(list.navigate(Direction::Last), Some(&"explanation"));
    }
    #[test]
    fn remapping_retains_identity_then_the_nearest_surviving_position() {
        let mut list = Navigation::new(Axis::Horizontal);
        list.reconcile(&[
            Entry::new(1, true),
            Entry::new(2, true),
            Entry::new(3, true),
        ]);
        assert_eq!(list.current(), None);
        assert!(list.set_current(&2));
        list.reconcile(&[
            Entry::new(3, true),
            Entry::new(1, true),
            Entry::new(2, false),
        ]);
        assert_eq!(list.current(), Some(&2));
        assert!(!list.can_activate());
        list.reconcile(&[Entry::new(3, true), Entry::new(1, true)]);
        assert_eq!(list.current(), Some(&1));
        assert!(!list.set_current(&99));
        assert_eq!(list.current(), Some(&1));
        list.reconcile(&[]);
        assert_eq!(list.current(), None);
    }
}
