//! Glaux's separate interpretation of SWE quality descriptions and associations.
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

pub mod binding;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualityError {
    Shape,
    UnsupportedComponent,
    InvalidReference,
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

/// The four members of the conceptual Quality union and unresolved associations.
/// The boxed contracts retain units, constraints, nil rules, exact source and
/// nested quality. No URI or extension metadata is fetched or normalized.
pub enum InlineQuality {
    Quantity(Box<ScalarContract>),
    QuantityRange(Box<RangeContract>),
    Category(Box<ScalarContract>),
    Text(Box<ScalarContract>),
    /// Preserved association only. Per-record resolution requires the complete
    /// source and an occurrence-aware QualityReferencePlan.
    Reference(QualityReference),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QualityReference {
    pub href: String,
    pub role: Option<String>,
    pub arcrole: Option<String>,
    pub title: Option<String>,
    source: Vec<u8>,
}

impl QualityReference {
    pub fn source(&self) -> &[u8] {
        &self.source
    }

    pub(crate) fn compile(raw: &RawValue) -> Result<Self, QualityError> {
        let value: Value = serde_json::from_str(raw.get()).map_err(|_| QualityError::Shape)?;
        let object = value.as_object().ok_or(QualityError::Shape)?;
        if ["type", "value", "quality", "constraint", "nilValues", "uom"]
            .iter()
            .any(|key| object.contains_key(*key))
        {
            return Err(QualityError::InvalidReference);
        }
        let string = |key: &str| -> Result<Option<String>, QualityError> {
            object
                .get(key)
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .ok_or(QualityError::InvalidReference)
                })
                .transpose()
        };
        let href = string("href")?.ok_or(QualityError::InvalidReference)?;
        if href.is_empty() || href == "#" {
            return Err(QualityError::InvalidReference);
        }
        crate::scalar::check_format(&href, "uri-reference")
            .map_err(|_| QualityError::InvalidReference)?;
        let role = string("role")?;
        let arcrole = string("arcrole")?;
        for uri in role.iter().chain(arcrole.iter()) {
            crate::scalar::check_format(uri, "uri").map_err(|_| QualityError::InvalidReference)?;
        }
        let title = string("title")?;
        if title.as_ref().is_some_and(String::is_empty) {
            return Err(QualityError::InvalidReference);
        }
        Ok(Self {
            href,
            role,
            arcrole,
            title,
            source: raw.get().as_bytes().to_vec(),
        })
    }
}

impl InlineQuality {
    pub fn source(&self) -> &[u8] {
        match self {
            Self::Quantity(value) | Self::Category(value) | Self::Text(value) => value.source(),
            Self::QuantityRange(value) => value.source(),
            Self::Reference(value) => value.source(),
        }
    }

    pub fn availability(&self) -> QualityAvailability {
        match self {
            Self::Reference(_) => QualityAvailability::Missing,
            Self::Quantity(value) | Self::Category(value) | Self::Text(value) => {
                match value.inline_value() {
                    None => QualityAvailability::Missing,
                    Some(value) if value.nil_reason.is_some() => QualityAvailability::Nil,
                    Some(_) => QualityAvailability::Supplied,
                }
            }
            Self::QuantityRange(value) => match value.inline_value() {
                None => QualityAvailability::Missing,
                Some(value)
                    if value
                        .endpoints
                        .iter()
                        .any(|value| value.nil_reason.is_some()) =>
                {
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
            let source: Value =
                serde_json::from_str(entry.get()).map_err(|_| QualityError::Shape)?;
            let source = source.as_object().ok_or(QualityError::Shape)?;
            if source.contains_key("href") {
                compiled.push(InlineQuality::Reference(QualityReference::compile(&entry)?));
                continue;
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
