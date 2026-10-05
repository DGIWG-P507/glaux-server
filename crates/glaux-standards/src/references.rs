//! Bounded local SWE component-reference resolution over an exact source artifact.
//!
//! This stage checks structure and reference topology, not complete component
//! semantics or payload occurrence rules. The scalar/aggregate compilers remain
//! separate; immutable contract assembly composes those stages in task 2.1.12.
//! Only published component association slots are followed. Quality, encoding
//! paths, units, reference frames and arbitrary extension links are not resolved.
//! No network or filesystem resolver is exposed or used.
use std::{collections::BTreeMap, ops::Range};

use serde_json::{Value, value::RawValue};

use crate::{
    aggregate::AggregateError,
    array::{self, ArrayOptions, CountReferenceSchema, SourceValidation},
    validation::{self, Contract, StructuralValidator},
};

/// Glaux resource budgets, additional to the shared JSON syntax budgets.
pub const MAX_COMPONENTS: usize = 512;
pub const MAX_REFERENCES: usize = 512;
/// Root has depth one; both containment and resolved references add one level.
pub const MAX_TRAVERSAL_DEPTH: usize = 32;
/// Every visited component occurrence counts, including repeated local targets.
pub const MAX_TRAVERSAL_STEPS: usize = 4096;

/// Bounded diagnostics contain no supplied identifiers or source content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceError {
    Syntax(validation::Failure),
    Structure,
    UnsupportedComponent,
    InvalidReference,
    InvalidTarget,
    InlineElementValue,
    DuplicateId,
    UnresolvedLocal,
    Cycle,
    ComponentLimit,
    ReferenceLimit,
    DepthLimit,
    TraversalLimit,
    AdaptationSourceChanged,
}

/// One inline declaration, indexed in declaration order. An inline elementCount
/// has kind Count even when its permitted source form omits the type member.
#[derive(Debug)]
pub struct ComponentNode {
    pub id: Option<String>,
    pub name: Option<String>,
    pub kind: Contract,
    /// JSON pointer to the declaration; the root pointer is empty.
    pub path: String,
    /// Byte range within ComponentGraph::source(), without surrounding whitespace.
    pub source: Range<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceKind {
    Component,
    ElementCount,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceTarget {
    Local(usize),
    /// Preserved metadata only; no target content, existence or meaning is known.
    Nonlocal,
}

#[derive(Debug)]
pub struct ComponentReference {
    pub owner: usize,
    pub kind: ReferenceKind,
    pub name: Option<String>,
    pub path: String,
    pub source: Range<usize>,
    pub href: String,
    pub role: Option<String>,
    pub arcrole: Option<String>,
    pub title: Option<String>,
    pub target: ReferenceTarget,
}

/// A source index with resolved edges, never an expanded copy of the graph.
#[derive(Debug)]
pub struct ComponentGraph {
    source: Vec<u8>,
    source_validation: SourceValidation,
    components: Vec<ComponentNode>,
    references: Vec<ComponentReference>,
    traversal: Vec<usize>,
}

impl ComponentGraph {
    pub fn resolve(
        validator: &StructuralValidator,
        input: &[u8],
        options: ArrayOptions,
    ) -> Result<Self, ReferenceError> {
        // Bound the complete original document before indexing or schema work.
        let value = validation::parse(input).map_err(ReferenceError::Syntax)?;
        let kind = component_kind(&value)?;
        let source_validation = structure(validator, kind, input, options)?;
        let raw: &RawValue =
            serde_json::from_slice(input).map_err(|_| ReferenceError::Structure)?;
        let mut index = Index {
            input,
            validator,
            options: ArrayOptions {
                count_reference_schema: match source_validation {
                    SourceValidation::Original => CountReferenceSchema::Original,
                    SourceValidation::CountReferenceCorrection => {
                        CountReferenceSchema::DisjointReferenceCorrection
                    }
                },
            },
            components: Vec::new(),
            references: Vec::new(),
            edges: Vec::new(),
            has_value: Vec::new(),
            ids: BTreeMap::new(),
        };
        index.component(&value, raw, String::new(), kind)?;
        index.resolve_targets()?;
        let traversal = index.traverse()?;
        Ok(Self {
            source: input.to_vec(),
            source_validation,
            components: index.components,
            references: index.references,
            traversal,
        })
    }

    pub fn source(&self) -> &[u8] {
        &self.source
    }

    pub fn source_validation(&self) -> SourceValidation {
        self.source_validation
    }

    /// Root is index zero. Inline children retain array declaration order;
    /// choiceValue precedes items, and elementCount precedes elementType.
    pub fn components(&self) -> &[ComponentNode] {
        &self.components
    }

    pub fn references(&self) -> &[ComponentReference] {
        &self.references
    }

    /// Depth-first component occurrences, including repeated local targets.
    /// A nonlocal edge has no available target and adds no component occurrence.
    pub fn traversal(&self) -> &[usize] {
        &self.traversal
    }

    pub fn component_source(&self, index: usize) -> Option<&[u8]> {
        self.components
            .get(index)
            .and_then(|component| self.source.get(component.source.clone()))
    }

    pub fn reference_source(&self, index: usize) -> Option<&[u8]> {
        self.references
            .get(index)
            .and_then(|reference| self.source.get(reference.source.clone()))
    }
}

#[derive(Clone, Copy)]
enum EdgeTarget {
    Component(usize),
    Reference(usize),
}

struct Edge {
    target: EdgeTarget,
    slot: Slot,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Slot {
    Component,
    ElementType,
    ElementCount,
}

struct Index<'a> {
    input: &'a [u8],
    validator: &'a StructuralValidator,
    options: ArrayOptions,
    components: Vec<ComponentNode>,
    references: Vec<ComponentReference>,
    edges: Vec<Vec<Edge>>,
    has_value: Vec<bool>,
    ids: BTreeMap<String, usize>,
}

impl Index<'_> {
    fn component(
        &mut self,
        value: &Value,
        raw: &RawValue,
        path: String,
        kind: Contract,
    ) -> Result<usize, ReferenceError> {
        if self.components.len() >= MAX_COMPONENTS {
            return Err(ReferenceError::ComponentLimit);
        }
        let id = optional_string(value, "id")?;
        let node = self.components.len();
        if let Some(id) = &id
            && self.ids.insert(id.clone(), node).is_some()
        {
            return Err(ReferenceError::DuplicateId);
        }
        self.components.push(ComponentNode {
            id,
            name: value.get("name").and_then(Value::as_str).map(str::to_owned),
            kind,
            path: path.clone(),
            source: self.source_range(raw)?,
        });
        self.edges.push(Vec::new());
        self.has_value.push(
            value.get("value").is_some()
                || (matches!(kind, Contract::DataArray | Contract::Matrix)
                    && value.get("values").is_some()),
        );
        let raw_members: BTreeMap<String, &RawValue> =
            serde_json::from_str(raw.get()).map_err(|_| ReferenceError::Structure)?;
        if kind == Contract::DataChoice
            && let Some(selector) = value.get("choiceValue")
        {
            let selector_raw = raw_members
                .get("choiceValue")
                .ok_or(ReferenceError::Structure)?;
            let child = self.component(
                selector,
                selector_raw,
                format!("{path}/choiceValue"),
                Contract::Category,
            )?;
            self.edges[node].push(Edge {
                target: EdgeTarget::Component(child),
                slot: Slot::Component,
            });
        }
        if matches!(kind, Contract::DataArray | Contract::Matrix) {
            // The original schema permits omission; payload semantics are still
            // the responsibility of the existing array compiler.
            if let Some(count) = value.get("elementCount") {
                let count_raw = raw_members
                    .get("elementCount")
                    .ok_or(ReferenceError::Structure)?;
                let count_path = format!("{path}/elementCount");
                let target = if count.get("href").is_some() {
                    self.reference(
                        node,
                        count,
                        count_raw,
                        count_path,
                        ReferenceKind::ElementCount,
                    )?
                } else {
                    if count
                        .get("type")
                        .is_some_and(|kind| kind.as_str() != Some("Count"))
                    {
                        return Err(ReferenceError::InvalidTarget);
                    }
                    EdgeTarget::Component(self.component(
                        count,
                        count_raw,
                        count_path,
                        Contract::Count,
                    )?)
                };
                self.edges[node].push(Edge {
                    target,
                    slot: Slot::ElementCount,
                });
            }
            let element = value.get("elementType").ok_or(ReferenceError::Structure)?;
            let element_raw = raw_members
                .get("elementType")
                .ok_or(ReferenceError::Structure)?;
            let target = self.child(node, element, element_raw, format!("{path}/elementType"))?;
            self.edges[node].push(Edge {
                target,
                slot: Slot::ElementType,
            });
        }
        let member = match kind {
            Contract::DataRecord => Some("fields"),
            Contract::Vector => Some("coordinates"),
            Contract::DataChoice => Some("items"),
            _ => None,
        };
        if let Some(member) = member {
            let children = value
                .get(member)
                .and_then(Value::as_array)
                .ok_or(ReferenceError::Structure)?;
            let raw_children: Vec<&RawValue> = serde_json::from_str(
                raw_members
                    .get(member)
                    .ok_or(ReferenceError::Structure)?
                    .get(),
            )
            .map_err(|_| ReferenceError::Structure)?;
            for (index, (child, child_raw)) in children.iter().zip(raw_children).enumerate() {
                let child_path = format!("{path}/{member}/{index}");
                let target = if kind == Contract::Vector {
                    // Coordinates have no association branch. An unrelated href
                    // extension on an inline scalar is not a component link.
                    EdgeTarget::Component(self.component(
                        child,
                        child_raw,
                        child_path,
                        component_kind(child)?,
                    )?)
                } else {
                    self.child(node, child, child_raw, child_path)?
                };
                self.edges[node].push(Edge {
                    target,
                    slot: Slot::Component,
                });
            }
        }
        Ok(node)
    }

    fn child(
        &mut self,
        owner: usize,
        value: &Value,
        raw: &RawValue,
        path: String,
    ) -> Result<EdgeTarget, ReferenceError> {
        let kind = component_kind(value);
        if let Ok(kind) = kind
            && (value.get("href").is_none()
                || structure(self.validator, kind, raw.get().as_bytes(), self.options).is_ok())
        {
            // The enclosing oneOf already selected exactly one schema branch.
            // An inline component can have an unknown href extension that does
            // not itself satisfy AssociationAttributeGroup.
            return Ok(EdgeTarget::Component(
                self.component(value, raw, path, kind)?,
            ));
        }
        if value.get("href").is_some() {
            return self.reference(owner, value, raw, path, ReferenceKind::Component);
        }
        Err(ReferenceError::UnsupportedComponent)
    }

    fn reference(
        &mut self,
        owner: usize,
        value: &Value,
        raw: &RawValue,
        path: String,
        kind: ReferenceKind,
    ) -> Result<EdgeTarget, ReferenceError> {
        if self.references.len() >= MAX_REFERENCES {
            return Err(ReferenceError::ReferenceLimit);
        }
        // Retain the existing count compiler's explicit mixed-count rejection.
        // General component associations keep schema-permitted extensions.
        if kind == ReferenceKind::ElementCount
            && [
                "type",
                "value",
                "constraint",
                "nilValues",
                "referenceFrame",
                "axisID",
            ]
            .iter()
            .any(|member| value.get(member).is_some())
        {
            return Err(ReferenceError::InvalidReference);
        }
        let href = optional_string(value, "href")?.ok_or(ReferenceError::InvalidReference)?;
        if href.is_empty() || href == "#" {
            return Err(ReferenceError::InvalidReference);
        }
        let index = self.references.len();
        self.references.push(ComponentReference {
            owner,
            kind,
            name: value.get("name").and_then(Value::as_str).map(str::to_owned),
            path,
            source: self.source_range(raw)?,
            href,
            role: optional_string(value, "role")?,
            arcrole: optional_string(value, "arcrole")?,
            title: optional_string(value, "title")?,
            target: ReferenceTarget::Nonlocal,
        });
        Ok(EdgeTarget::Reference(index))
    }

    fn source_range(&self, raw: &RawValue) -> Result<Range<usize>, ReferenceError> {
        // Borrowed RawValue points into the submitted byte slice. Keep ranges,
        // not copies of each nested source, and use no unsafe pointer access.
        let start = (raw.get().as_ptr() as usize)
            .checked_sub(self.input.as_ptr() as usize)
            .ok_or(ReferenceError::Structure)?;
        let end = start
            .checked_add(raw.get().len())
            .filter(|end| *end <= self.input.len())
            .ok_or(ReferenceError::Structure)?;
        Ok(start..end)
    }

    fn resolve_targets(&mut self) -> Result<(), ReferenceError> {
        for reference in &mut self.references {
            if let Some(fragment) = reference.href.strip_prefix('#') {
                let id = decode_fragment(fragment)?;
                let target = *self.ids.get(&id).ok_or(ReferenceError::UnresolvedLocal)?;
                if reference.kind == ReferenceKind::ElementCount
                    && self.components[target].kind != Contract::Count
                {
                    return Err(ReferenceError::InvalidTarget);
                }
                reference.target = ReferenceTarget::Local(target);
            }
        }
        Ok(())
    }

    fn traverse(&self) -> Result<Vec<usize>, ReferenceError> {
        struct Frame {
            node: usize,
            next: usize,
            element: bool,
        }
        let mut active = vec![false; self.components.len()];
        active[0] = true;
        let mut frames = vec![Frame {
            node: 0,
            next: 0,
            element: false,
        }];
        let mut traversal = vec![0];
        while let Some(frame) = frames.last_mut() {
            let Some(edge) = self.edges[frame.node].get(frame.next) else {
                active[frame.node] = false;
                frames.pop();
                continue;
            };
            frame.next += 1;
            let target = match edge.target {
                EdgeTarget::Component(target) => target,
                EdgeTarget::Reference(reference) => match self.references[reference].target {
                    ReferenceTarget::Local(target) => target,
                    ReferenceTarget::Nonlocal => continue,
                },
            };
            if edge.slot == Slot::ElementType
                && self.components[frame.node].kind == Contract::Matrix
                && !matches!(
                    self.components[target].kind,
                    Contract::Matrix | Contract::Count | Contract::Quantity | Contract::Time
                )
            {
                return Err(ReferenceError::InvalidTarget);
            }
            // Fixed counts are metadata, not inline element values. An array
            // nested in an element descriptor may still declare its own size.
            let element = match edge.slot {
                Slot::Component => frame.element,
                Slot::ElementType => true,
                Slot::ElementCount => false,
            };
            if element && self.has_value[target] {
                return Err(ReferenceError::InlineElementValue);
            }
            if active[target] {
                return Err(ReferenceError::Cycle);
            }
            if frames.len() >= MAX_TRAVERSAL_DEPTH {
                return Err(ReferenceError::DepthLimit);
            }
            if traversal.len() >= MAX_TRAVERSAL_STEPS {
                return Err(ReferenceError::TraversalLimit);
            }
            traversal.push(target);
            active[target] = true;
            frames.push(Frame {
                node: target,
                next: 0,
                element,
            });
        }
        Ok(traversal)
    }
}

fn optional_string(value: &Value, member: &str) -> Result<Option<String>, ReferenceError> {
    value
        .get(member)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or(ReferenceError::Structure)
        })
        .transpose()
}

fn structure(
    validator: &StructuralValidator,
    kind: Contract,
    input: &[u8],
    options: ArrayOptions,
) -> Result<SourceValidation, ReferenceError> {
    if matches!(
        kind,
        Contract::DataRecord
            | Contract::Vector
            | Contract::DataChoice
            | Contract::DataArray
            | Contract::Matrix
    ) {
        array::structure(validator, kind, input, options).map_err(|error| match error {
            AggregateError::AdaptationSourceChanged => ReferenceError::AdaptationSourceChanged,
            _ => ReferenceError::Structure,
        })
    } else {
        validator
            .validate(kind, input)
            .map_err(|_| ReferenceError::Structure)?;
        Ok(SourceValidation::Original)
    }
}

fn component_kind(value: &Value) -> Result<Contract, ReferenceError> {
    match value.get("type").and_then(Value::as_str) {
        Some("Boolean") => Ok(Contract::Boolean),
        Some("Text") => Ok(Contract::Text),
        Some("Category") => Ok(Contract::Category),
        Some("Count") => Ok(Contract::Count),
        Some("Quantity") => Ok(Contract::Quantity),
        Some("Time") => Ok(Contract::Time),
        Some("CategoryRange") => Ok(Contract::CategoryRange),
        Some("CountRange") => Ok(Contract::CountRange),
        Some("QuantityRange") => Ok(Contract::QuantityRange),
        Some("TimeRange") => Ok(Contract::TimeRange),
        Some("DataRecord") => Ok(Contract::DataRecord),
        Some("Vector") => Ok(Contract::Vector),
        Some("DataChoice") => Ok(Contract::DataChoice),
        Some("DataArray") => Ok(Contract::DataArray),
        Some("Matrix") => Ok(Contract::Matrix),
        Some("Geometry") => Ok(Contract::Geometry),
        _ => Err(ReferenceError::UnsupportedComponent),
    }
}

fn decode_fragment(fragment: &str) -> Result<String, ReferenceError> {
    let mut decoded = Vec::with_capacity(fragment.len());
    let mut bytes = fragment.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = bytes
                .next()
                .and_then(hex)
                .ok_or(ReferenceError::InvalidReference)?;
            let low = bytes
                .next()
                .and_then(hex)
                .ok_or(ReferenceError::InvalidReference)?;
            decoded.push(high * 16 + low);
        } else {
            decoded.push(byte);
        }
    }
    String::from_utf8(decoded).map_err(|_| ReferenceError::InvalidReference)
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
