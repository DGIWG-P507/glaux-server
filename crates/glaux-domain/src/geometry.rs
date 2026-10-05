//! SWE Geometry is a component value, not a scalar, Feature or transformed shape.
use crate::{aggregate::AggregateMetadata, numeric::ExactNumber};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometryKind {
    Point,
    MultiPoint,
    LineString,
    MultiLineString,
    Polygon,
    MultiPolygon,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Position {
    /// Supplied axis order and exact numeric lexemes, including signed zero.
    pub ordinates: Vec<ExactNumber>,
}

impl Position {
    /// The third ordinate, without a unit/datum conversion or an inferred value.
    pub fn height(&self) -> Option<&ExactNumber> {
        self.ordinates.get(2)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GeometryValue {
    Point(Position),
    MultiPoint(Vec<Position>),
    LineString(Vec<Position>),
    MultiLineString(Vec<Vec<Position>>),
    Polygon(Vec<Vec<Position>>),
    MultiPolygon(Vec<Vec<Vec<Position>>>),
}

impl GeometryValue {
    pub fn kind(&self) -> GeometryKind {
        match self {
            Self::Point(_) => GeometryKind::Point,
            Self::MultiPoint(_) => GeometryKind::MultiPoint,
            Self::LineString(_) => GeometryKind::LineString,
            Self::MultiLineString(_) => GeometryKind::MultiLineString,
            Self::Polygon(_) => GeometryKind::Polygon,
            Self::MultiPolygon(_) => GeometryKind::MultiPolygon,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeometryConstraint {
    /// None leaves kinds unrestricted; an explicit empty list allows no kind.
    pub geom_types: Option<Vec<GeometryKind>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeometryNilDeclaration {
    /// Textual declaration retained independently of the object-only value form.
    pub value: String,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeometryComponent {
    pub metadata: AggregateMetadata,
    pub srs: String,
    pub constraint: Option<GeometryConstraint>,
    pub nil_values: Vec<GeometryNilDeclaration>,
    pub value: Option<GeometryValue>,
}
