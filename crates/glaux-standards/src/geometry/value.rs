//! Shape checks after the complete original value schema and bounded parse.
use std::collections::BTreeMap;

use glaux_domain::{
    geometry::{GeometryValue, Position},
    numeric::ExactNumber,
};
use serde_json::value::RawValue;

use super::{GeometryError, SrsCheck};

pub(super) fn parse(
    input: &[u8],
    srs: &SrsCheck,
) -> Result<(GeometryValue, Option<Vec<ExactNumber>>), GeometryError> {
    let raw: BTreeMap<String, Box<RawValue>> =
        serde_json::from_slice(input).map_err(|_| GeometryError::Structure)?;
    // RFC 7946 section 7.1 reserves these root members for Features and
    // FeatureCollections. Section 6.1 leaves foreign-member descendants alone.
    if ["geometry", "properties", "features"]
        .iter()
        .any(|member| raw.contains_key(*member))
    {
        return Err(GeometryError::Structure);
    }
    let kind: String = serde_json::from_str(raw.get("type").ok_or(GeometryError::Structure)?.get())
        .map_err(|_| GeometryError::Structure)?;
    let coordinates = raw.get("coordinates").ok_or(GeometryError::Coordinates)?;
    let mut dimension = match srs {
        SrsCheck::KnownDimensions(dimension) => Some(usize::from(*dimension)),
        SrsCheck::Unresolved(_) => None,
    };
    let value = match kind.as_str() {
        "Point" => GeometryValue::Point(position(coordinates, &mut dimension)?),
        "MultiPoint" => GeometryValue::MultiPoint(positions(coordinates, &mut dimension)?),
        "LineString" => GeometryValue::LineString(line(coordinates, &mut dimension)?),
        "MultiLineString" => GeometryValue::MultiLineString(
            array(coordinates)?
                .iter()
                .map(|row| line(row, &mut dimension))
                .collect::<Result<_, _>>()?,
        ),
        "Polygon" => GeometryValue::Polygon(polygon(coordinates, &mut dimension)?),
        "MultiPolygon" => GeometryValue::MultiPolygon(
            array(coordinates)?
                .iter()
                .map(|shape| polygon(shape, &mut dimension))
                .collect::<Result<_, _>>()?,
        ),
        _ => return Err(GeometryError::Structure),
    };
    let bbox = raw
        .get("bbox")
        .map(|bbox| {
            let bbox = numbers(bbox).map_err(|_| GeometryError::Bbox)?;
            if !matches!(bbox.len(), 4 | 6) || dimension.is_some_and(|n| bbox.len() != 2 * n) {
                return Err(GeometryError::Bbox);
            }
            // Coverage/topology and antimeridian interpretation are not inferred.
            Ok(bbox)
        })
        .transpose()?;
    Ok((value, bbox))
}

fn array(raw: &RawValue) -> Result<Vec<Box<RawValue>>, GeometryError> {
    serde_json::from_str(raw.get()).map_err(|_| GeometryError::Coordinates)
}

fn numbers(raw: &RawValue) -> Result<Vec<ExactNumber>, GeometryError> {
    array(raw)?
        .iter()
        .map(|number| {
            ExactNumber::parse_json_number(number.get()).map_err(|_| GeometryError::Coordinates)
        })
        .collect()
}

fn position(raw: &RawValue, dimension: &mut Option<usize>) -> Result<Position, GeometryError> {
    let ordinates = numbers(raw)?;
    if !matches!(ordinates.len(), 2 | 3) || dimension.is_some_and(|n| n != ordinates.len()) {
        return Err(GeometryError::Dimension);
    }
    *dimension = Some(ordinates.len());
    Ok(Position { ordinates })
}

fn positions(raw: &RawValue, dimension: &mut Option<usize>) -> Result<Vec<Position>, GeometryError> {
    array(raw)?
        .iter()
        .map(|value| position(value, dimension))
        .collect()
}

fn line(raw: &RawValue, dimension: &mut Option<usize>) -> Result<Vec<Position>, GeometryError> {
    let positions = positions(raw, dimension)?;
    if positions.len() < 2 {
        return Err(GeometryError::Coordinates);
    }
    Ok(positions)
}

fn polygon(raw: &RawValue, dimension: &mut Option<usize>) -> Result<Vec<Vec<Position>>, GeometryError> {
    array(raw)?
        .iter()
        .map(|ring| {
            let positions = positions(ring, dimension)?;
            // Equality is exact numeric equality, not identical JSON spelling.
            if positions.len() < 4 || positions.first() != positions.last() {
                return Err(GeometryError::Ring);
            }
            Ok(positions)
        })
        .collect()
}
