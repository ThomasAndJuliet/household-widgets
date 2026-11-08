// SPDX-License-Identifier: GPL-3.0-only

//! The one undo history of the household's text boxes, multi-line
//! ([`crate::TextBox`]) and one-line ([`crate::ShortTextBox`]), so that
//! Ctrl+Z goes back by the same steps in each: the states a box can go back
//! and forward to, the run of typing or deleting going on, and the app's
//! own part of a step (household rule 19b). [`Undoable`] is what a box
//! with this history offers, written once for both.

use std::collections::VecDeque;

/// The steps Ctrl+Z can go back
pub const STEPS: usize = 300;

/// The most text a box's history keeps, in bytes, the oldest steps going
/// first: a long draft pasted again and again stays in bounds
pub const BYTES: usize = 8 << 20;

/// A step undone or redone that has the app's part: the part, for the app
/// to undo or redo its own side of it
#[derive(Clone, Debug, PartialEq)]
pub enum Turn<P> {
    Undone(P),
    Redone(P),
}

/// A state kept, the app's part of its step, and the state's size in bytes
#[derive(Debug)]
struct Step<S, P> {
    state: S,
    part: Option<P>,
    bytes: usize,
}

/// The run of typing or deleting going on, which the next edit of the same
/// kind joins as part of the same step
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Run {
    /// Typing; whether the last character was a space or new line
    Typing {
        space: bool,
    },
    Deleting,
}

impl Run {
    /// Whether `next`, after this, is part of the same step: typing goes on
    /// until a space after a word starts the next step
    fn joined_by(self, next: Run) -> bool {
        match (self, next) {
            (Run::Typing { space }, Run::Typing { space: next }) => space || !next,
            (Run::Deleting, Run::Deleting) => true,
            _ => false,
        }
    }
}

/// A box's history: each step the state before it (to undo) or after it (to
/// redo), at most [`STEPS`] steps and [`BYTES`] bytes. `S` is the box's
/// whole state, so that going back is putting it back.
#[doc(hidden)]
#[derive(Debug)]
pub struct History<S, P> {
    undo: VecDeque<Step<S, P>>,
    redo: Vec<Step<S, P>>,
    /// What `undo` and `redo` hold, in bytes
    bytes: usize,
    /// The run of typing or deleting going on, if any
    run: Option<Run>,
}

impl<S, P> Default for History<S, P> {
    fn default() -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            bytes: 0,
            run: None,
        }
    }
}

impl<S, P> History<S, P> {
    /// The steps there are to undo and to redo
    pub(crate) fn len(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }
}

impl<S, P: Clone> History<S, P> {
    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// An edit of kind `next` (`None`: a paste, a cut, a word deleted,
    /// anything else) is to be made: whether it is part of the step going
    /// on. Typing over a selection starts a step, which the typing after it
    /// joins. The edit's kind is then the run going on
    pub(crate) fn joins(&mut self, next: Option<Run>, selected: bool) -> bool {
        let joins = !selected
            && match (self.run, next) {
                (Some(going), Some(next)) => going.joined_by(next),
                _ => false,
            };
        self.run = next;
        joins
    }

    /// The run going on has ended: the caret moved, or an edit changed
    /// nothing
    pub(crate) fn end_run(&mut self) {
        self.run = None;
    }

    /// A step taken from `before`, of `bytes`, with the app's `part` of it:
    /// what Redo would have brought back is gone
    pub(crate) fn record(&mut self, before: S, bytes: usize, part: Option<P>) {
        self.drop_redo();
        self.bytes += bytes;
        self.undo.push_back(Step {
            state: before,
            part,
            bytes,
        });
        while self.undo.len() > STEPS || (self.bytes > BYTES && self.undo.len() > 1) {
            let oldest = self.undo.pop_front().expect("a step");
            self.bytes -= oldest.bytes;
        }
    }

    /// Back a step (`back`) or forward one from `now`, of `bytes`: the
    /// state to put back and the app's part of the step
    pub(crate) fn turn(&mut self, back: bool, now: S, bytes: usize) -> Option<(S, Option<P>)> {
        let step = match back {
            true => self.undo.pop_back(),
            false => self.redo.pop(),
        }?;
        self.bytes = self.bytes + bytes - step.bytes;
        let kept = Step {
            state: now,
            part: step.part.clone(),
            bytes,
        };
        match back {
            true => self.redo.push(kept),
            false => self.undo.push_back(kept),
        }
        self.run = None;
        Some((step.state, step.part))
    }

    /// What Redo would have brought back is gone, the steps before kept
    pub(crate) fn drop_redo(&mut self) {
        self.bytes -= self.redo.drain(..).map(|step| step.bytes).sum::<usize>();
    }

    fn part(step: Option<&Step<S, P>>) -> Option<&P> {
        step.and_then(|step| step.part.as_ref())
    }
}

/// A text box with the household's undo history: [`crate::TextBox`] and
/// [`crate::ShortTextBox`]. What it offers is here, once for both; its
/// hidden methods are each box's own state, for these to work on.
pub trait Undoable {
    /// The box's action, as its widget publishes it
    type Action;
    /// The app's own part of a step (rule 19b)
    type Part: Clone;
    /// Where the caret goes when the app sets the text
    type Caret;
    #[doc(hidden)]
    type State;
    /// The action that undoes, and the one that redoes
    const UNDO: Self::Action;
    const REDO: Self::Action;

    #[doc(hidden)]
    fn history(&self) -> &History<Self::State, Self::Part>;
    #[doc(hidden)]
    fn history_mut(&mut self) -> &mut History<Self::State, Self::Part>;
    /// The box's whole state now, and its size in bytes
    #[doc(hidden)]
    fn state(&self) -> (Self::State, usize);
    /// The box's whole state put back
    #[doc(hidden)]
    fn restore(&mut self, state: Self::State);
    /// The text set by the app, the caret at `caret`; no step
    #[doc(hidden)]
    fn set(&mut self, text: &str, caret: Self::Caret);
    /// Whether the box's text is `text`
    #[doc(hidden)]
    fn holds(&self, text: &str) -> bool;
    /// The app replaced the text with the same text, the caret at `caret`
    #[doc(hidden)]
    fn unchanged(&mut self, caret: Self::Caret);
    /// Whether the box keeps a history at all: a password's keeps none
    #[doc(hidden)]
    fn keeps_history(&self) -> bool {
        true
    }

    /// Whether Undo would change anything
    fn can_undo(&self) -> bool {
        self.history().can_undo()
    }

    /// Whether Redo would change anything
    fn can_redo(&self) -> bool {
        self.history().can_redo()
    }

    /// The app's part of the step Undo would take back, if it has one
    fn undo_part(&self) -> Option<&Self::Part> {
        History::part(self.history().undo.back())
    }

    /// The app's part of the step Redo would bring back, if it has one
    fn redo_part(&self) -> Option<&Self::Part> {
        History::part(self.history().redo.last())
    }

    /// The steps with an app's part can no longer be undone or redone (the
    /// app's side of them changed): the history ends at the nearest of them
    /// either way, the steps of the text alone between kept. Going back
    /// past one would bring back text its part took elsewhere
    fn forget_parts(&mut self) {
        let history = self.history_mut();
        if let Some(at) = history.undo.iter().rposition(|step| step.part.is_some()) {
            history.bytes -= history
                .undo
                .drain(..=at)
                .map(|step| step.bytes)
                .sum::<usize>();
        }
        if let Some(at) = history.redo.iter().rposition(|step| step.part.is_some()) {
            history.bytes -= history
                .redo
                .drain(..=at)
                .map(|step| step.bytes)
                .sum::<usize>();
        }
    }

    /// The text replaced by the app, the caret at `caret`: one step, which
    /// Undo takes back. The same text again is no step: a text box's caret
    /// still goes to `caret`, and a one-line field's is left where it is
    /// (a reload putting the term back must not move what is typed)
    fn replace(&mut self, text: &str, caret: Self::Caret) {
        if self.holds(text) {
            self.unchanged(caret);
            return;
        }
        let (before, bytes) = self.state();
        self.set(text, caret);
        if self.keeps_history() {
            self.history_mut().record(before, bytes, None);
        }
    }

    /// The text replaced by the app as one step with `part`, the app's own
    /// part of it, which Undo and Redo hand back as a [`Turn`]: always a
    /// step, the text changed or not
    fn replace_with(&mut self, text: &str, caret: Self::Caret, part: Self::Part) {
        let (before, bytes) = self.state();
        self.set(text, caret);
        if self.keeps_history() {
            self.history_mut().record(before, bytes, Some(part));
        }
    }

    /// Back a step (`back`, Undo) or forward one (Redo), if there is one;
    /// a step with the app's part hands it back
    #[doc(hidden)]
    fn turn(&mut self, back: bool) -> Option<Turn<Self::Part>> {
        let can = match back {
            true => self.can_undo(),
            false => self.can_redo(),
        };
        if !can {
            return None;
        }
        let (now, bytes) = self.state();
        let (state, part) = self.history_mut().turn(back, now, bytes)?;
        self.restore(state);
        part.map(|part| match back {
            true => Turn::Undone(part),
            false => Turn::Redone(part),
        })
    }
}

/// The history's bounds, on states that say their own size
#[cfg(test)]
mod tests {
    use super::{BYTES, History, STEPS};

    #[test]
    fn the_history_keeps_at_most_its_steps_and_bytes() {
        let mut history: History<usize, ()> = History::default();
        for _ in 0..STEPS + 5 {
            history.record(1, 1, None);
        }
        assert_eq!(history.undo.len(), STEPS);
        assert_eq!(history.bytes, STEPS);
        // Big states: the oldest go first, the newest is always kept
        let mut history: History<usize, ()> = History::default();
        for n in 1..=8 {
            history.record(BYTES / 4 + n, BYTES / 4 + n, None);
        }
        assert_eq!(history.undo.len(), 3);
        assert_eq!(
            history.undo.front().map(|step| step.state),
            Some(BYTES / 4 + 6)
        );
        history.record(2 * BYTES, 2 * BYTES, None);
        assert_eq!(
            history.undo.len(),
            1,
            "one step bigger than the bound is kept alone"
        );
        // Undo and Redo move bytes between the two, and a new step drops Redo
        let mut history: History<usize, ()> = History::default();
        history.record(10, 10, None);
        let (before, _) = history.turn(true, 20, 20).expect("a step");
        assert_eq!((before, history.bytes), (10, 20));
        history.record(5, 5, None);
        assert_eq!(history.bytes, 5);
        assert!(history.redo.is_empty());
    }
}
