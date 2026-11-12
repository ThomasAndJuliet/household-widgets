// SPDX-License-Identifier: GPL-3.0-only

//! Component-owned presentations, resolved before native measurement.
use super::Child;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Emphasis {
    #[default]
    Normal,
    Emphasised,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Detail {
    #[default]
    Concise,
    Verbose,
}

/// Presentation is independent of the requested compaction level.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Presentation {
    pub emphasis: Emphasis,
    pub detail: Detail,
}

/// Omitted axes fall back independently to immediate defaults, then family
/// defaults. Explicit Normal and Concise override an emphasised/verbose default.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct PresentationRequest {
    pub emphasis: Option<Emphasis>,
    pub detail: Option<Detail>,
}

impl From<Presentation> for PresentationRequest {
    fn from(value: Presentation) -> Self {
        Self {
            emphasis: Some(value.emphasis),
            detail: Some(value.detail),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnsupportedPresentation {
    pub requested: Presentation,
}

/// An explicit catalogue of supported pairs, including the family's default.
/// A legacy adapter may declare only its existing presentation. Fallbacks are
/// caller policy: an unsupported pair never silently selects another pair.
#[derive(Clone, Debug)]
pub struct PresentationFamily {
    default: Presentation,
    supported: Vec<Presentation>,
}

impl PresentationFamily {
    pub fn new(default: Presentation, supported: Vec<Presentation>) -> Self {
        assert!(
            supported.contains(&default),
            "family default must be supported"
        );
        Self { default, supported }
    }

    pub fn default_presentation(&self) -> Presentation {
        self.default
    }

    pub fn supported(&self) -> &[Presentation] {
        &self.supported
    }

    pub fn resolve(
        &self,
        instance: PresentationRequest,
        immediate_defaults: PresentationRequest,
    ) -> Result<Presentation, UnsupportedPresentation> {
        let requested = Presentation {
            emphasis: instance
                .emphasis
                .or(immediate_defaults.emphasis)
                .unwrap_or(self.default.emphasis),
            detail: instance
                .detail
                .or(immediate_defaults.detail)
                .unwrap_or(self.default.detail),
        };
        if self.supported.contains(&requested) {
            Ok(requested)
        } else {
            Err(UnsupportedPresentation { requested })
        }
    }

    /// Build once from already prepared data. The returned ordinary Child binds
    /// this resolution to all subsequent level queries and verified rendering.
    /// It deliberately supplies no implicit defaults to descendants: composites
    /// resolve each immediate child/slot explicitly, within a candidate if its
    /// inclusion policy depends on the candidate form.
    /// Unsupported requests do not invoke `build`.
    pub fn prepare<'a, Message: 'a>(
        &self,
        instance: PresentationRequest,
        immediate_defaults: PresentationRequest,
        build: impl FnOnce(Presentation) -> Child<'a, Message>,
    ) -> Result<Child<'a, Message>, UnsupportedPresentation> {
        let presentation = self.resolve(instance, immediate_defaults)?;
        let mut child = build(presentation);
        child.presentation = Some(presentation);
        child.refresh_preparation();
        Ok(child)
    }
}
