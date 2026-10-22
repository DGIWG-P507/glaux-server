//! Glaux's separate interpretation of SWE inline quality descriptions.
//!
//! The pinned JSON schemas leave `quality` open; a structural pass does not
//! validate it. These assertions retain their own contracts and supplied values.
//! Their subject is the containing scalar/range contract, not an invented field.
use serde_json::{Value, value::RawValue};

use crate::{
    range::{RangeContract, RangeOptions},
    scalar::ScalarContract,
    validation::StructuralValidator,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualityError {
    Shape,
    UnsupportedComponent,
    ReferenceUnsupported,
    InvalidComponent,
}

/// Availability of a supplied assertion, not a verdict about the subject.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualityAvailability {
    Missing,
    /// A declared nil sentinel, including a range with either endpoint nil.
    Nil,
    /// A supplied, locally validated value; never an automatic quality "pass".
    Supplied,
}

/// Only the four members of the conceptual Quality union are inline assertions.
/// The boxed contracts retain units, constraints, nil rules, exact source and
/// nested quality. No URI or extension metadata is fetched or normalized.
pub enum InlineQuality {
    Quantity(Box<ScalarContract>),
    QuantityRange(Box<RangeContract>),
    Category(Box<ScalarContract>),
    Text(Box<ScalarContract>),
}

impl InlineQuality {
    pub fn source(&self) -> &[u8] {
        match self {
            Self::Quantity(value) | Self::Category(value) | Self::Text(value) => value.source(),
            Self::QuantityRange(value) => value.source(),
        }
    }

    pub fn availability(&self) -> QualityAvailability {
        match self {
            Self::Quantity(value) | Self::Category(value) | Self::Text(value) => {
                match value.inline_value() {
                    None => QualityAvailability::Missing,
                    Some(value) if value.nil_reason.is_some() => QualityAvailability::Nil,
                    Some(_) => QualityAvailability::Supplied,
                }
            }
            Self::QuantityRange(value) => match value.inline_value() {
                None => QualityAvailability::Missing,
                Some(value) if value.endpoints.iter().any(|value| value.nil_reason.is_some()) => {
                    QualityAvailability::Nil
                }
                Some(_) => QualityAvailability::Supplied,
            },
        }
    }
}

/// Absent and explicitly empty quality stay distinguishable. Neither supplies
/// an assertion, and neither invents zero uncertainty or a successful outcome.
#[derive(Default)]
pub struct QualityAssertions {
    present: bool,
    entries: Vec<InlineQuality>,
}

impl QualityAssertions {
    pub fn is_present(&self) -> bool {
        self.present
    }

    pub fn entries(&self) -> &[InlineQuality] {
        &self.entries
    }

    /// Called only after the containing document's bounded parser. Those byte,
    /// depth and node limits include the entire nested-quality subtree, before
    /// recursion starts; parsing an entry cannot reset that enclosing budget.
    pub(crate) fn compile(
        validator: &StructuralValidator,
        raw: Option<&RawValue>,
    ) -> Result<Self, QualityError> {
        let Some(raw) = raw else {
            return Ok(Self::default());
        };
        let entries: Vec<Box<RawValue>> =
            serde_json::from_str(raw.get()).map_err(|_| QualityError::Shape)?;
        let mut compiled = Vec::with_capacity(entries.len());
        for entry in entries {
            let source: Value = serde_json::from_str(entry.get()).map_err(|_| QualityError::Shape)?;
            let source = source.as_object().ok_or(QualityError::Shape)?;
            // Reference binding belongs to the next task. Do not silently accept
            // it as an inline component, even when a type is supplied as well.
            if source.contains_key("href") {
                return Err(QualityError::ReferenceUnsupported);
            }
            let bytes = entry.get().as_bytes();
            let assertion = match source.get("type").and_then(Value::as_str) {
                Some("Quantity") => InlineQuality::Quantity(Box::new(
                    ScalarContract::compile(validator, bytes)
                        .map_err(|_| QualityError::InvalidComponent)?,
                )),
                Some("QuantityRange") => InlineQuality::QuantityRange(Box::new(
                    RangeContract::compile(validator, bytes, RangeOptions::default())
                        .map_err(|_| QualityError::InvalidComponent)?,
                )),
                Some("Category") => InlineQuality::Category(Box::new(
                    ScalarContract::compile(validator, bytes)
                        .map_err(|_| QualityError::InvalidComponent)?,
                )),
                Some("Text") => InlineQuality::Text(Box::new(
                    ScalarContract::compile(validator, bytes)
                        .map_err(|_| QualityError::InvalidComponent)?,
                )),
                _ => return Err(QualityError::UnsupportedComponent),
            };
            compiled.push(assertion);
        }
        Ok(Self {
            present: true,
            entries: compiled,
        })
    }
}

#[cfg(test)]
mod tests;
