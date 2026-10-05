//! Bounded SWE Geometry descriptions and their six-kind GeoJSON value form.
//!
//! No Feature mapping, SRS retrieval, axis swap, transformation, topology engine,
//! WKT/WKB parsing or complete result codec is provided here.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

use glaux_domain::geometry::{
    GeometryComponent, GeometryConstraint, GeometryKind, GeometryNilDeclaration, GeometryValue,
};
use serde_json::{Value, value::RawValue};

use crate::{
    aggregate, scalar,
    validation::{self, Contract, StructuralValidator},
};

mod value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometryError {
    Syntax(validation::Failure),
    Structure,
    UnsupportedComponent,
    UnsupportedFeature,
    Metadata,
    ConstraintViolation,
    Coordinates,
    Dimension,
    Ring,
    Bbox,
    DuplicateNilValue,
    NilLimit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SrsCheck {
    /// A fixed sourced dimension binding, not coordinate/axis transformation.
    KnownDimensions(u8),
    /// The URI was checked but its dimensional/axis meaning is not resolved.
    Unresolved(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedGeometryValue {
    pub value: GeometryValue,
    pub srs: String,
    pub srs_check: SrsCheck,
    pub bbox: Option<Vec<glaux_domain::numeric::ExactNumber>>,
    /// Exact value-object bytes; permitted foreign members are not discarded.
    pub source: Vec<u8>,
}

impl CheckedGeometryValue {
    pub fn source(&self) -> &[u8] {
        &self.source
    }
}

pub struct GeometryContract {
    component: GeometryComponent,
    source: Vec<u8>,
    srs_check: SrsCheck,
    inline: Option<CheckedGeometryValue>,
}

impl GeometryContract {
    pub fn compile(validator: &StructuralValidator, input: &[u8]) -> Result<Self, GeometryError> {
        let source = validation::parse(input).map_err(GeometryError::Syntax)?;
        if source.get("type").and_then(Value::as_str) != Some("Geometry") {
            return Err(GeometryError::UnsupportedComponent);
        }
        validator
            .validate(Contract::Geometry, input)
            .map_err(|_| GeometryError::Structure)?;
        if source.get("quality").is_some() {
            return Err(GeometryError::UnsupportedFeature);
        }
        let metadata = aggregate::metadata(&source).map_err(|_| GeometryError::Metadata)?;
        let srs = source
            .get("srs")
            .and_then(Value::as_str)
            .ok_or(GeometryError::Metadata)?
            .to_owned();
        let constraint = source
            .get("constraint")
            .map(|constraint| {
                let geom_types = constraint
                    .get("geomTypes")
                    .map(|types| {
                        types
                            .as_array()
                            .ok_or(GeometryError::Structure)?
                            .iter()
                            .map(|kind| {
                                kind.as_str()
                                    .and_then(kind_from_name)
                                    .ok_or(GeometryError::Structure)
                            })
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?;
                Ok::<_, GeometryError>(GeometryConstraint { geom_types })
            })
            .transpose()?;
        let nil_values = nil_declarations(&source)?;
        let srs_check = srs_check(&srs);
        let mut compiled = Self {
            component: GeometryComponent {
                metadata,
                srs,
                constraint,
                nil_values,
                value: None,
            },
            source: input.to_vec(),
            srs_check,
            inline: None,
        };
        let raw: BTreeMap<String, Box<RawValue>> =
            serde_json::from_slice(input).map_err(|_| GeometryError::Structure)?;
        if let Some(value) = raw.get("value") {
            let checked = compiled.check_value(value.get().as_bytes())?;
            compiled.component.value = Some(checked.value.clone());
            compiled.inline = Some(checked);
        }
        Ok(compiled)
    }

    pub fn component(&self) -> &GeometryComponent {
        &self.component
    }

    pub fn source(&self) -> &[u8] {
        &self.source
    }

    pub fn srs_check(&self) -> &SrsCheck {
        &self.srs_check
    }

    pub fn inline_value(&self) -> Option<&CheckedGeometryValue> {
        self.inline.as_ref()
    }

    pub fn nil_declarations(&self) -> &[GeometryNilDeclaration] {
        &self.component.nil_values
    }

    /// Object-only geometry meaning. Textual nil declarations do not silently
    /// become string/null geometries or establish a later codec's nil mapping.
    pub fn check_value(&self, input: &[u8]) -> Result<CheckedGeometryValue, GeometryError> {
        let source = validation::parse(input).map_err(GeometryError::Syntax)?;
        if !value_validator()?.is_valid(&source) {
            return Err(GeometryError::Structure);
        }
        let (value, bbox) = value::parse(input, &self.srs_check)?;
        if let Some(constraint) = &self.component.constraint
            && let Some(kinds) = &constraint.geom_types
            && !kinds.contains(&value.kind())
        {
            return Err(GeometryError::ConstraintViolation);
        }
        Ok(CheckedGeometryValue {
            value,
            bbox,
            srs: self.component.srs.clone(),
            srs_check: self.srs_check.clone(),
            source: input.to_vec(),
        })
    }
}

fn value_validator() -> Result<&'static jsonschema::Validator, GeometryError> {
    static VALIDATOR: OnceLock<Result<jsonschema::Validator, String>> = OnceLock::new();
    VALIDATOR
        .get_or_init(|| {
            let catalog = validation::catalog()?;
            validation::compile_component(&catalog, Contract::GeometryValue)
        })
        .as_ref()
        .map_err(|_| GeometryError::Structure)
}

fn kind_from_name(name: &str) -> Option<GeometryKind> {
    Some(match name {
        "Point" => GeometryKind::Point,
        "MultiPoint" => GeometryKind::MultiPoint,
        "LineString" => GeometryKind::LineString,
        "MultiLineString" => GeometryKind::MultiLineString,
        "Polygon" => GeometryKind::Polygon,
        "MultiPolygon" => GeometryKind::MultiPolygon,
        _ => return None,
    })
}

fn srs_check(srs: &str) -> SrsCheck {
    // Fixed dimension-only bindings: OGC CRS definitions, RFC 7946 section 4,
    // and the EPSG maintainers' 4326/4979 geographic-2D/geographic-3D distinction.
    // No URI normalization/alias inference or coordinate reordering is applied.
    match srs {
        "http://www.opengis.net/def/crs/OGC/1.3/CRS84"
        | "urn:ogc:def:crs:OGC::CRS84"
        | "http://www.opengis.net/def/crs/EPSG/0/4326" => SrsCheck::KnownDimensions(2),
        "http://www.opengis.net/def/crs/OGC/0/CRS84h"
        | "http://www.opengis.net/def/crs/EPSG/0/4979" => SrsCheck::KnownDimensions(3),
        _ => SrsCheck::Unresolved(srs.to_owned()),
    }
}

fn nil_declarations(source: &Value) -> Result<Vec<GeometryNilDeclaration>, GeometryError> {
    let Some(nil_values) = source.get("nilValues") else {
        return Ok(Vec::new());
    };
    let nil_values = nil_values.as_array().ok_or(GeometryError::Structure)?;
    if nil_values.len() > scalar::MAX_NIL_DECLARATIONS {
        return Err(GeometryError::NilLimit);
    }
    let mut seen = BTreeSet::new();
    nil_values
        .iter()
        .map(|nil| {
            let value = nil
                .get("value")
                .and_then(Value::as_str)
                .ok_or(GeometryError::Structure)?;
            let reason = nil
                .get("reason")
                .and_then(Value::as_str)
                .ok_or(GeometryError::Structure)?;
            scalar::check_format(reason, "uri").map_err(|_| GeometryError::Metadata)?;
            if !seen.insert(value) {
                return Err(GeometryError::DuplicateNilValue);
            }
            Ok(GeometryNilDeclaration {
                value: value.to_owned(),
                reason: reason.to_owned(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
