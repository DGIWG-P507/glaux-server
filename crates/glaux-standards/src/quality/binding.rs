//! Quality associations over already-decoded component occurrences.
//!
//! This is not a complete contract/codec plan: it checks quality and its target
//! values, not every unrelated field, array cardinality or encoding descriptor.
//! There is no caller-selected target or occurrence identifier to trust.
use crate::{
    array::ArrayOptions,
    choice::{ComponentValue, NamedValue},
    range::{CheckedRangeValue, RangeContract, RangeOptions},
    references::{
        ComponentGraph, Edge, EdgeTarget, MAX_TRAVERSAL_DEPTH, MAX_TRAVERSAL_STEPS, ReferenceError,
        ReferenceKind, ReferenceTarget, Slot,
    },
    scalar::{CheckedScalarValue, ScalarContract},
    validation::{Contract, StructuralValidator},
};

use super::QualityAvailability;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualityBindingError {
    Reference(ReferenceError),
    InvalidTarget,
    InvalidValue,
    ValueShape,
    AmbiguousOccurrence,
    Limit,
}

/// Indices are declared record/vector/choice slots or array element indices,
/// never reordered input positions. The root address is empty.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentOccurrence {
    pub component: usize,
    pub path: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum QualityReferenceValue {
    Missing,
    /// Metadata was preserved; this is not a locally resolved missing value.
    Nonlocal,
    Scalar(CheckedScalarValue),
    Range(CheckedRangeValue),
}

impl QualityReferenceValue {
    pub fn availability(&self) -> QualityAvailability {
        match self {
            Self::Missing | Self::Nonlocal => QualityAvailability::Missing,
            Self::Scalar(value) if value.nil_reason.is_some() => QualityAvailability::Nil,
            Self::Range(value)
                if value
                    .endpoints
                    .iter()
                    .any(|value| value.nil_reason.is_some()) =>
            {
                QualityAvailability::Nil
            }
            Self::Scalar(_) | Self::Range(_) => QualityAvailability::Supplied,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoundQualityReference {
    /// Index in graph().references(), retaining exact association metadata.
    pub reference: usize,
    pub subject: ComponentOccurrence,
    /// A known omitted slot still has an address. An inactive branch, absent
    /// repetition or nonlocal association has no available target occurrence.
    pub target: Option<ComponentOccurrence>,
    pub value: QualityReferenceValue,
}

enum TargetContract {
    Scalar(Box<ScalarContract>),
    Range(Box<RangeContract>),
}

pub struct QualityReferencePlan {
    graph: ComponentGraph,
    targets: Vec<Option<TargetContract>>,
    attached: Vec<Vec<usize>>,
}

impl QualityReferencePlan {
    pub fn compile(
        validator: &StructuralValidator,
        input: &[u8],
        options: ArrayOptions,
    ) -> Result<Self, QualityBindingError> {
        let graph = ComponentGraph::resolve(validator, input, options)
            .map_err(QualityBindingError::Reference)?;
        let mut payload = vec![false; graph.components().len()];
        let mut pending = vec![0];
        while let Some(node) = pending.pop() {
            if payload[node] {
                continue;
            }
            payload[node] = true;
            for edge in payload_edges(&graph, node) {
                if let Some(child) = target(&graph, edge) {
                    pending.push(child);
                }
            }
        }
        let mut targets = (0..graph.components().len())
            .map(|_| None)
            .collect::<Vec<_>>();
        for reference in graph.references() {
            if reference.kind != ReferenceKind::Quality {
                continue;
            }
            let ReferenceTarget::Local(node) = reference.target else {
                continue;
            };
            // A metadata-only declaration has no encoded field/record occurrence.
            if !payload[node] {
                return Err(QualityBindingError::InvalidTarget);
            }
            if targets[node].is_some() {
                continue;
            }
            let source = graph
                .component_source(node)
                .ok_or(QualityBindingError::InvalidTarget)?;
            targets[node] = Some(match graph.components()[node].kind {
                Contract::Quantity | Contract::Category | Contract::Text => {
                    TargetContract::Scalar(Box::new(
                        ScalarContract::compile(validator, source)
                            .map_err(|_| QualityBindingError::InvalidTarget)?,
                    ))
                }
                Contract::QuantityRange => TargetContract::Range(Box::new(
                    RangeContract::compile(validator, source, RangeOptions::default())
                        .map_err(|_| QualityBindingError::InvalidTarget)?,
                )),
                _ => return Err(QualityBindingError::InvalidTarget),
            });
        }
        let mut attached = vec![Vec::new(); graph.components().len()];
        for (node, list) in attached.iter_mut().enumerate() {
            // Inline quality remains metadata attached to this payload subject;
            // nested associations use its occurrence, never hidden payload slots.
            let mut pending = vec![EdgeTarget::Component(node)];
            while let Some(target) = pending.pop() {
                match target {
                    EdgeTarget::Reference(reference) => list.push(reference),
                    EdgeTarget::Component(owner) => {
                        for edge in graph.edges[owner].iter().rev() {
                            if edge.slot == Slot::Quality {
                                pending.push(edge.target);
                            }
                        }
                    }
                }
            }
        }
        Ok(Self {
            graph,
            targets,
            attached,
        })
    }

    pub fn graph(&self) -> &ComponentGraph {
        &self.graph
    }

    /// Does not mutate or fill the input, fall back to static descriptor values,
    /// fetch references, aggregate quality, or convert an unavailable value to pass.
    pub fn bind(
        &self,
        input: &ComponentValue<'_>,
    ) -> Result<Vec<BoundQualityReference>, QualityBindingError> {
        crate::choice::value::budget(input).map_err(|_| QualityBindingError::Limit)?;
        let mut occurrences = Vec::new();
        self.walk(0, Some(input), Vec::new(), Vec::new(), &mut occurrences)?;
        let mut result = Vec::new();
        for occurrence in &occurrences {
            for &reference_index in &self.attached[occurrence.address.component] {
                if result.len() >= MAX_TRAVERSAL_STEPS {
                    return Err(QualityBindingError::Limit);
                }
                let reference = &self.graph.references()[reference_index];
                let (target, value) = match reference.target {
                    ReferenceTarget::Nonlocal => (None, QualityReferenceValue::Nonlocal),
                    ReferenceTarget::Local(node) => {
                        let mut candidates = occurrences.iter().filter(|candidate| {
                            candidate.address.component == node
                                && compatible(&occurrence.context, &candidate.context)
                        });
                        let candidate = candidates.next();
                        if candidates.next().is_some() {
                            return Err(QualityBindingError::AmbiguousOccurrence);
                        }
                        match candidate {
                            Some(candidate) => {
                                (Some(candidate.address.clone()), candidate.value.clone())
                            }
                            None => (None, QualityReferenceValue::Missing),
                        }
                    }
                };
                result.push(BoundQualityReference {
                    reference: reference_index,
                    subject: ComponentOccurrence {
                        component: reference.owner,
                        path: occurrence.address.path.clone(),
                    },
                    target,
                    value,
                });
            }
        }
        Ok(result)
    }

    fn walk(
        &self,
        node: usize,
        input: Option<&ComponentValue<'_>>,
        path: Vec<usize>,
        mut context: Vec<ComponentOccurrence>,
        occurrences: &mut Vec<Occurrence>,
    ) -> Result<(), QualityBindingError> {
        if path.len() >= MAX_TRAVERSAL_DEPTH || occurrences.len() >= MAX_TRAVERSAL_STEPS {
            return Err(QualityBindingError::Limit);
        }
        let kind = self.graph.components()[node].kind;
        let address = ComponentOccurrence {
            component: node,
            path: path.clone(),
        };
        let value = match (self.targets[node].as_ref(), input) {
            (Some(TargetContract::Scalar(contract)), Some(ComponentValue::ScalarJson(raw))) => {
                QualityReferenceValue::Scalar(
                    contract
                        .check_value(raw)
                        .map_err(|_| QualityBindingError::InvalidValue)?,
                )
            }
            (Some(TargetContract::Range(contract)), Some(ComponentValue::RangeJson(raw))) => {
                QualityReferenceValue::Range(
                    contract
                        .check_value(raw)
                        .map_err(|_| QualityBindingError::InvalidValue)?,
                )
            }
            (Some(_), Some(_)) => return Err(QualityBindingError::ValueShape),
            _ => QualityReferenceValue::Missing,
        };
        occurrences.push(Occurrence {
            address: address.clone(),
            context: context.clone(),
            value,
        });
        match kind {
            Contract::DataRecord | Contract::Vector | Contract::DataChoice => {
                let values = match (kind, input) {
                    (_, None) => &[][..],
                    (Contract::DataRecord, Some(ComponentValue::Record(values)))
                    | (Contract::Vector, Some(ComponentValue::Vector(values)))
                    | (Contract::DataChoice, Some(ComponentValue::Choice(values))) => *values,
                    _ => return Err(QualityBindingError::ValueShape),
                };
                if kind == Contract::DataChoice && input.is_some() && values.len() != 1 {
                    return Err(QualityBindingError::ValueShape);
                }
                context.push(address);
                let edges = payload_edges(&self.graph, node).collect::<Vec<_>>();
                let slots = self.match_members(&edges, values)?;
                for (index, (edge, value)) in edges.iter().zip(slots).enumerate() {
                    if kind == Contract::DataChoice && value.is_none() {
                        continue;
                    }
                    let Some(child) = target(&self.graph, edge) else {
                        if value.is_some() {
                            return Err(QualityBindingError::ValueShape);
                        }
                        continue;
                    };
                    let mut child_path = path.clone();
                    child_path.push(index);
                    self.walk(child, value, child_path, context.clone(), occurrences)?;
                }
            }
            Contract::DataArray | Contract::Matrix => {
                let values = match input {
                    None => &[][..],
                    Some(ComponentValue::Array(values)) => *values,
                    _ => return Err(QualityBindingError::ValueShape),
                };
                let edge = payload_edges(&self.graph, node)
                    .next()
                    .ok_or(QualityBindingError::ValueShape)?;
                let child = target(&self.graph, edge);
                for (index, value) in values.iter().enumerate() {
                    let child = child.ok_or(QualityBindingError::ValueShape)?;
                    let mut child_path = path.clone();
                    child_path.push(index);
                    let mut child_context = context.clone();
                    // Each repetition is its own context, not just the array's
                    // container address. Otherwise adjacent records could mix.
                    child_context.push(ComponentOccurrence {
                        component: node,
                        path: child_path.clone(),
                    });
                    self.walk(child, Some(value), child_path, child_context, occurrences)?;
                }
            }
            _ => {
                let shape = match kind {
                    Contract::CategoryRange
                    | Contract::CountRange
                    | Contract::QuantityRange
                    | Contract::TimeRange => {
                        input.is_none() || matches!(input, Some(ComponentValue::RangeJson(_)))
                    }
                    Contract::Geometry => {
                        input.is_none() || matches!(input, Some(ComponentValue::GeometryJson(_)))
                    }
                    _ => input.is_none() || matches!(input, Some(ComponentValue::ScalarJson(_))),
                };
                if !shape {
                    return Err(QualityBindingError::ValueShape);
                }
            }
        }
        Ok(())
    }

    fn match_members<'a, 'b>(
        &self,
        edges: &[&Edge],
        values: &'a [NamedValue<'b>],
    ) -> Result<Vec<Option<&'a ComponentValue<'b>>>, QualityBindingError> {
        let names = edges
            .iter()
            .map(|edge| match edge.target {
                EdgeTarget::Component(node) => self.graph.components()[node].name.as_deref(),
                EdgeTarget::Reference(reference) => {
                    self.graph.references()[reference].name.as_deref()
                }
            })
            .collect::<Vec<_>>();
        for (index, name) in names.iter().enumerate() {
            if name.is_none() || names[..index].contains(name) {
                return Err(QualityBindingError::ValueShape);
            }
        }
        let mut slots = vec![None; edges.len()];
        for value in values {
            let index = names
                .iter()
                .position(|name| *name == Some(value.name))
                .ok_or(QualityBindingError::ValueShape)?;
            if slots[index].replace(&value.value).is_some() {
                return Err(QualityBindingError::ValueShape);
            }
        }
        Ok(slots)
    }
}

struct Occurrence {
    address: ComponentOccurrence,
    context: Vec<ComponentOccurrence>,
    value: QualityReferenceValue,
}

fn payload_edges(graph: &ComponentGraph, node: usize) -> impl Iterator<Item = &Edge> {
    graph.edges[node]
        .iter()
        .filter(|edge| matches!(edge.slot, Slot::Component | Slot::ElementType))
}

fn target(graph: &ComponentGraph, edge: &Edge) -> Option<usize> {
    match edge.target {
        EdgeTarget::Component(node) => Some(node),
        EdgeTarget::Reference(reference) => match graph.references()[reference].target {
            ReferenceTarget::Local(node) => Some(node),
            ReferenceTarget::Nonlocal => None,
        },
    }
}

fn compatible(subject: &[ComponentOccurrence], target: &[ComponentOccurrence]) -> bool {
    // A common container definition must denote the same occurrence. Merely
    // sharing an ID or the final field name is insufficient across repetitions.
    subject.iter().all(|left| {
        target
            .iter()
            .all(|right| left.component != right.component || left.path == right.path)
    })
}

#[cfg(test)]
mod tests;
