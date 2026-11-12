// SPDX-License-Identifier: GPL-3.0-only

//! Pure source facts in a plan, exposed only by native accepted-box traversal.
use super::*;
use std::any::Any;

/// Application-owned stable identity of one source instance.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceId(pub u64);

/// Family-local logical information identity, independent of widget shape.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InformationId(pub &'static str);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LossKind {
    Shortened,
    Omitted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InformationLoss {
    pub identity: InformationId,
    pub kind: LossKind,
}

/// Facts about this prepared snapshot's selected form. This is presentation
/// loss, not a generic status, pending-value or preview classification. An empty
/// losses vector explicitly means this source has no reported information loss.
#[derive(Clone, Debug)]
pub struct SourceReport {
    pub source: SourceId,
    pub revision: u64,
    pub presentation: Option<Presentation>,
    pub level: Level,
    pub size: Size,
    pub losses: Vec<InformationLoss>,
    pub(super) owner: PreparedId,
}

impl PartialEq for SourceReport {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
            && self.revision == other.revision
            && self.presentation == other.presentation
            && self.level == other.level
            && self.size == other.size
            && self.losses == other.losses
    }
}

/// Published by `Operation::custom` only on the retained, verified native box.
/// Consumers must collect afresh after accepted layout and match the source
/// revision before choosing recovery contents. A missing source has no report.
#[derive(Clone, Debug, PartialEq)]
pub struct AcceptedSourceReport {
    pub report: SourceReport,
    pub bounds: Rectangle,
}

/// A fresh collector represents one traversal of the current native tree.
/// Trial queries cannot modify it or publish facts into an application registry.
#[derive(Default)]
pub struct SourceReports {
    reports: Vec<AcceptedSourceReport>,
}

impl SourceReports {
    pub fn reports(&self) -> &[AcceptedSourceReport] {
        &self.reports
    }

    pub fn into_reports(self) -> Vec<AcceptedSourceReport> {
        self.reports
    }
}

impl Operation for SourceReports {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn custom(
        &mut self,
        _: Option<&cosmic::iced::advanced::widget::Id>,
        _: Rectangle,
        state: &mut dyn Any,
    ) {
        if let Some(report) = state.downcast_ref::<AcceptedSourceReport>() {
            self.reports.push(report.clone());
        }
    }
}

pub(super) type InformationQuery<'a> =
    Rc<dyn Fn(Level, &GeometryPlan) -> Vec<InformationLoss> + 'a>;

#[derive(Clone)]
pub(super) struct SourceInformation<'a> {
    pub source: SourceId,
    pub revision: u64,
    pub report: InformationQuery<'a>,
}

/// A verification wrapper, not a second engine or an observer callback. Facts
/// become observable only when an operation traverses the retained native tree.
/// All wrappers of a committed surface share its verification flag, so a failed
/// sibling or decoration prevents publication from the entire rejected surface.
pub(super) struct VerifiedBox<'a, Message> {
    pub content: Element<'a, Message>,
    pub reports: Vec<SourceReport>,
    pub binding_matches: bool,
    pub agreement: Rc<Cell<bool>>,
}

impl<Message> Widget<Message, Theme, Renderer> for VerifiedBox<'_, Message> {
    forward_widget!(Message, Theme, Renderer; children, diff, size, draw, update, mouse_interaction, overlay, drag_destinations);

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        if !self.binding_matches {
            self.agreement.set(false);
        }
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        if self.agreement.get() && self.binding_matches {
            for report in &self.reports {
                // Custom operations receive a disposable copy; they cannot
                // mutate the prepared plan or later publication.
                let mut accepted = AcceptedSourceReport {
                    report: report.clone(),
                    bounds: layout.bounds(),
                };
                operation.custom(None, layout.bounds(), &mut accepted);
            }
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                operation,
            );
        }
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
