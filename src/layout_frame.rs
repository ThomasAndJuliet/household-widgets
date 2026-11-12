// SPDX-License-Identifier: GPL-3.0-only

//! Explicit chrome around an ordinary immediate child, using its existing plan.
use super::*;
use cosmic::iced::Padding;

pub(super) struct Frame<'a, Message> {
    pub content: Child<'a, Message>,
    padding: Padding,
    decorate: ViewMap<'a, Message>,
}

/// Add declared native chrome without flattening a box into a fixed-size leaf.
/// The decorator must add exactly `padding` around its supplied content; native
/// verification rejects disagreement. Include border/shadow allowance here or
/// in another declared outer frame. All geometry, forms, minima, source facts
/// and requested levels of the immediate child remain part of the same engine.
/// Hidden content omits the entire frame and its spacing.
pub fn framed_child<'a, Message: 'a>(
    content: Child<'a, Message>,
    padding: impl Into<Padding>,
    decorate: impl Fn(Element<'a, Message>) -> Element<'a, Message> + 'a,
) -> Child<'a, Message> {
    let padding = padding.into();
    assert!(
        [padding.left, padding.right, padding.top, padding.bottom]
            .iter()
            .all(|value| value.is_finite() && *value >= 0.0)
    );
    let frame = Rc::new(Frame {
        content,
        padding,
        decorate: Rc::new(decorate),
    });
    let measure = frame.clone();
    let mut result = child(|_| widget::space().into());
    result.query = Rc::new(move |level, renderer| {
        let inner = measure.content.query(level, renderer);
        let chrome = Size::new(measure.padding.x(), measure.padding.y());
        MeasuredSize {
            preferred: Size::new(
                inner.preferred.width + chrome.width,
                inner.preferred.height + chrome.height,
            ),
            minimum: Size::new(
                if measure.content.weight == 0.0 {
                    inner.preferred.width
                } else {
                    inner.minimum.width
                } + chrome.width,
                inner.minimum.height + chrome.height,
            ),
        }
    });
    result.frame = Some(frame);
    result
}

impl<'a, Message: 'a> Frame<'a, Message> {
    pub fn plan(
        &self,
        level: Level,
        renderer: &Renderer,
        width: f32,
        height: f32,
        grow_height: f32,
    ) -> Result<GeometryPlan, AllocationError> {
        let horizontal = self.padding.x();
        let vertical = self.padding.y();
        if width + FIT_EPSILON < horizontal {
            return Err(AllocationError::NoFit {
                required: horizontal,
                available: width,
            });
        }
        if height + FIT_EPSILON < vertical {
            return Err(AllocationError::NoFit {
                required: vertical,
                available: height,
            });
        }
        let mut content = self.content.clone();
        if grow_height > 0.0 {
            content.grow_height = grow_height;
        }
        let geometry = content.plan_width(
            level,
            renderer,
            (width - horizontal).max(0.0),
            (height - vertical).max(0.0),
        )?;
        let mut result = GeometryPlan::leaf(Size::new(
            geometry.size.width + horizontal,
            geometry.size.height + vertical,
        ));
        result.minimum_height = geometry.minimum_height + vertical;
        result.children.push(Placement {
            index: 0,
            position: cosmic::iced::Point::new(self.padding.left, self.padding.top),
            geometry,
        });
        Ok(result)
    }

    pub fn render(
        &self,
        level: Level,
        plan: &GeometryPlan,
        agreement: Rc<Cell<bool>>,
        grow_height: f32,
    ) -> Element<'a, Message> {
        let inner = &plan.children[0].geometry;
        let mut content = self.content.clone();
        if grow_height > 0.0 {
            content.grow_height = grow_height;
        }
        let native = content.render(level, inner, agreement.clone());
        let native = Element::new(VerifiedDecoration {
            content: native,
            expected: inner.size,
            agreement: agreement.clone(),
            check_constraints: true,
        });
        Element::new(VerifiedDecoration {
            content: (self.decorate)(native),
            expected: plan.size,
            agreement,
            check_constraints: false,
        })
    }
}
