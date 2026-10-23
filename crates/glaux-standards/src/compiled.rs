//! Immutable, bounded assembly of component meaning and descriptor evidence.
//!
//! Original bytes are never rewritten or substituted by the parsed meaning.
//! Local bindings are indices, not expanded copies of referenced subtrees.
//! Encoding descriptors are checked descriptions, not executable wire codecs.
use std::collections::{BTreeMap, BTreeSet};

use glaux_domain::{aggregate::AggregateMetadata, array::ElementCount};
use serde_json::{Value, value::RawValue};
use sha2::{Digest, Sha256};

use crate::{
    aggregate::{self, AggregateError},
    array::{self, ArrayOptions},
    geometry::{GeometryContract, GeometryError},
    quality::binding::{QualityBindingError, QualityReferencePlan},
    range::{RangeContract, RangeError, RangeOptions},
    references::{ComponentGraph, EdgeTarget, MAX_TRAVERSAL_STEPS, ReferenceTarget, Slot},
    scalar::{ScalarContract, ScalarError},
    validation::{self, Contract, Encoding, StructuralValidator},
};

/// A compilation-wide retained-source/work budget, shared by every wrapper
/// member and frame context, derived from the existing byte and depth budgets.
pub const MAX_COMPILED_SOURCE_BYTES: usize = validation::MAX_BYTES * validation::MAX_DEPTH;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Descriptor {
    Component,
    ObservationJson,
    CommandJson,
    ObservationSwe,
    CommandSwe,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompileError {
    Syntax(validation::Failure),
    Structure,
    MediaType,
    Limit,
    Reference(QualityBindingError),
    Scalar(ScalarError),
    Range(RangeError),
    Geometry(GeometryError),
    Aggregate(AggregateError),
}

/// Only immutable borrows are exposed. No global cache or caller-owned backing
/// buffer can change a successfully compiled contract.
pub struct CompiledContract {
    source: Vec<u8>,
    media_type: String,
    sha256: [u8; 32],
    descriptor: Descriptor,
    source_validation: array::SourceValidation,
    plans: Vec<ComponentPlan>,
    encoding: Option<EncodingDescriptor>,
}

impl CompiledContract {
    pub fn compile(
        validator: &StructuralValidator,
        input: &[u8],
        media_type: &str,
        descriptor: Descriptor,
        options: ArrayOptions,
    ) -> Result<Self, CompileError> {
        // This is bounded source-artifact metadata, not an HTTP media-type
        // admission adapter or the separate obsFormat/commandFormat selector.
        if media_type.is_empty()
            || media_type.len() > 256
            || !media_type.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
        {
            return Err(CompileError::MediaType);
        }
        let value = validation::parse(input).map_err(CompileError::Syntax)?;
        let raw: BTreeMap<String, &RawValue> =
            serde_json::from_slice(input).map_err(|_| CompileError::Structure)?;
        let (wrapper, members): (Option<Contract>, &'static [&'static str]) = match descriptor {
            Descriptor::Component => (None, &[]),
            Descriptor::ObservationJson => (
                Some(Contract::ObservationJson),
                &["resultSchema", "parametersSchema"],
            ),
            Descriptor::CommandJson => (
                Some(Contract::CommandJson),
                &[
                    "parametersSchema",
                    "resultSchema",
                    "feasibilityResultSchema",
                ],
            ),
            Descriptor::ObservationSwe => (Some(Contract::ObservationSwe), &["recordSchema"]),
            Descriptor::CommandSwe => (Some(Contract::CommandSwe), &["recordSchema"]),
        };
        let wrapper_validation = if let Some(wrapper) = wrapper {
            // Validate the entire original wrapper, not a bag of passing
            // children. Only the existing explicitly selected count-reference
            // correction can qualify a failure of the original wrapper.
            Some(
                array::structure(validator, wrapper, input, options)
                    .map_err(CompileError::Aggregate)?,
            )
        } else {
            None
        };
        let encoding = if matches!(
            descriptor,
            Descriptor::ObservationSwe | Descriptor::CommandSwe
        ) {
            let encoding = raw.get("encoding").ok_or(CompileError::Structure)?;
            let format = match descriptor {
                Descriptor::ObservationSwe => "obsFormat",
                _ => "commandFormat",
            };
            let expected = match value.get(format).and_then(Value::as_str) {
                Some("application/swe+json") => Encoding::Json,
                Some("application/swe+text" | "application/swe+csv") => Encoding::Text,
                Some("application/swe+binary") => Encoding::Binary,
                _ => return Err(CompileError::Structure),
            };
            Some(EncodingDescriptor::compile(
                validator,
                encoding,
                Some(expected),
            )?)
        } else {
            None
        };
        let mut budget = Budget::default();
        let mut plans = Vec::new();
        if descriptor == Descriptor::Component {
            plans.push(ComponentPlan::compile(
                validator,
                input,
                None,
                options,
                &mut budget,
            )?);
        } else {
            for &member in members {
                if let Some(source) = raw.get(member) {
                    let result = ComponentPlan::compile(
                        validator,
                        source.get().as_bytes(),
                        Some(member),
                        options,
                        &mut budget,
                    );
                    // A failing later member must not publish the earlier,
                    // otherwise valid plans as a usable partial contract.
                    plans.push(result?); // No partially compiled wrapper escapes.
                }
            }
        }
        let source_validation =
            wrapper_validation.unwrap_or_else(|| plans[0].graph().source_validation());
        Ok(Self {
            source: input.to_vec(),
            media_type: media_type.to_owned(),
            sha256: Sha256::digest(input).into(),
            descriptor,
            source_validation,
            plans,
            encoding,
        })
    }

    pub fn source(&self) -> &[u8] {
        &self.source
    }
    pub fn media_type(&self) -> &str {
        &self.media_type
    }
    pub fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }
    pub fn descriptor(&self) -> Descriptor {
        self.descriptor
    }
    pub fn source_validation(&self) -> array::SourceValidation {
        self.source_validation
    }
    pub fn plans(&self) -> &[ComponentPlan] {
        &self.plans
    }
    pub fn encoding(&self) -> Option<&EncodingDescriptor> {
        self.encoding.as_ref()
    }
}

pub struct EncodingDescriptor {
    kind: Encoding,
    source: Vec<u8>,
    value: Value,
}

impl EncodingDescriptor {
    fn compile(
        validator: &StructuralValidator,
        raw: &RawValue,
        expected: Option<Encoding>,
    ) -> Result<Self, CompileError> {
        let value = validation::parse(raw.get().as_bytes()).map_err(CompileError::Syntax)?;
        let kind = match value.get("type").and_then(Value::as_str) {
            Some("JSONEncoding") => Encoding::Json,
            Some("TextEncoding") => Encoding::Text,
            Some("BinaryEncoding") => Encoding::Binary,
            _ => return Err(CompileError::Structure),
        };
        if expected.is_some_and(|expected| expected != kind) {
            return Err(CompileError::Structure);
        }
        validator
            .validate_encoding(kind, raw.get().as_bytes())
            .map_err(|_| CompileError::Structure)?;
        Ok(Self {
            kind,
            source: raw.get().as_bytes().to_vec(),
            value,
        })
    }

    pub fn kind(&self) -> Encoding {
        self.kind
    }
    pub fn source(&self) -> &[u8] {
        &self.source
    }
    pub fn value(&self) -> &Value {
        &self.value
    }
}

pub struct ComponentPlan {
    member: Option<&'static str>,
    quality: QualityReferencePlan,
    nodes: Vec<CompiledNode>,
}

impl ComponentPlan {
    fn compile(
        validator: &StructuralValidator,
        source: &[u8],
        member: Option<&'static str>,
        options: ArrayOptions,
        budget: &mut Budget,
    ) -> Result<Self, CompileError> {
        // Account graph and quality-source work before starting either stage.
        budget.source(source.len())?;
        let quality = QualityReferencePlan::compile(validator, source, options)
            .map_err(CompileError::Reference)?;
        let mut builder = Builder {
            validator,
            graph: quality.graph(),
            nodes: Vec::new(),
            indices: BTreeMap::new(),
            budget,
        };
        builder.node(0, None)?;
        let nodes = builder.nodes;
        Ok(Self {
            member,
            quality,
            nodes,
        })
    }

    pub fn member(&self) -> Option<&'static str> {
        self.member
    }
    pub fn graph(&self) -> &ComponentGraph {
        self.quality.graph()
    }
    pub fn nodes(&self) -> &[CompiledNode] {
        &self.nodes
    }
    pub fn quality(&self) -> &QualityReferencePlan {
        &self.quality
    }
    /// Nonlocal component/count bindings are retained metadata, not executable
    /// component definitions. Nonlocal quality is separately unavailable.
    pub fn has_unresolved_components(&self) -> bool {
        self.graph().references().iter().any(|reference| {
            reference.target == ReferenceTarget::Nonlocal
                && reference.kind != crate::references::ReferenceKind::Quality
        })
    }
}

pub enum CompiledSemantics {
    Scalar(Box<ScalarContract>),
    Range(Box<RangeContract>),
    Geometry(Box<GeometryContract>),
    Aggregate(AggregateDescription),
    ElementCount(ElementCount),
}

/// Fields are accessible only through an immutable compiled-node borrow.
pub struct AggregateDescription {
    pub kind: Contract,
    pub metadata: AggregateMetadata,
    pub reference_frame: Option<String>,
    pub local_frame: Option<String>,
    pub element_count: Option<ElementCount>,
}

pub struct CompiledNode {
    declaration: usize,
    inherited_frame: Option<String>,
    semantics: CompiledSemantics,
    bindings: Vec<CompiledBinding>,
    encoding: Option<EncodingDescriptor>,
}

impl CompiledNode {
    pub fn declaration(&self) -> usize {
        self.declaration
    }
    pub fn inherited_frame(&self) -> Option<&str> {
        self.inherited_frame.as_deref()
    }
    pub fn semantics(&self) -> &CompiledSemantics {
        &self.semantics
    }
    pub fn bindings(&self) -> &[CompiledBinding] {
        &self.bindings
    }
    pub fn encoding(&self) -> Option<&EncodingDescriptor> {
        self.encoding.as_ref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingSlot {
    Component,
    ElementType,
    ElementCount,
    Selector,
    Quality,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingTarget {
    /// Index in ComponentPlan::nodes(), including the effective frame context.
    Local(usize),
    /// Index in ComponentPlan::graph().references(); never fetched.
    Nonlocal(usize),
}

pub struct CompiledBinding {
    slot: BindingSlot,
    name: Option<String>,
    target: BindingTarget,
}

impl CompiledBinding {
    pub fn slot(&self) -> BindingSlot {
        self.slot
    }
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    pub fn target(&self) -> BindingTarget {
        self.target
    }
}

#[derive(Default)]
struct Budget {
    source_bytes: usize,
    contexts: usize,
}

impl Budget {
    fn source(&mut self, bytes: usize) -> Result<(), CompileError> {
        self.source_bytes = self
            .source_bytes
            .checked_add(bytes)
            .filter(|total| *total <= MAX_COMPILED_SOURCE_BYTES)
            .ok_or(CompileError::Limit)?;
        Ok(())
    }
    fn context(&mut self, bytes: usize) -> Result<(), CompileError> {
        self.contexts += 1;
        if self.contexts > MAX_TRAVERSAL_STEPS {
            return Err(CompileError::Limit);
        }
        self.source(bytes)
    }
}

struct Builder<'a> {
    validator: &'a StructuralValidator,
    graph: &'a ComponentGraph,
    nodes: Vec<CompiledNode>,
    indices: BTreeMap<(usize, Option<String>), usize>,
    budget: &'a mut Budget,
}

impl Builder<'_> {
    fn node(&mut self, declaration: usize, inherited: Option<&str>) -> Result<usize, CompileError> {
        let key = (declaration, inherited.map(str::to_owned));
        if let Some(index) = self.indices.get(&key) {
            return Ok(*index);
        }
        let input = self
            .graph
            .component_source(declaration)
            .ok_or(CompileError::Structure)?;
        self.budget.context(input.len())?;
        let source = validation::parse(input).map_err(CompileError::Syntax)?;
        let raw: BTreeMap<String, &RawValue> =
            serde_json::from_slice(input).map_err(|_| CompileError::Structure)?;
        let kind = self.graph.components()[declaration].kind;
        let own_frame = if matches!(
            kind,
            Contract::Vector
                | Contract::Matrix
                | Contract::Count
                | Contract::Quantity
                | Contract::Time
        ) {
            aggregate::optional_string(&source, "referenceFrame")
                .map_err(CompileError::Aggregate)?
        } else {
            None
        };
        // A declaration's role does not change when reached through an alias.
        // Typeless ElementCount objects are not ordinary scalar declarations.
        let count = self.graph.edges.iter().flatten().any(|edge| {
            edge.slot == Slot::ElementCount
                && matches!(edge.target, EdgeTarget::Component(target) if target == declaration)
        });
        let effective_frame = own_frame.as_deref().or(inherited);
        let semantics = if count {
            let raw: &RawValue =
                serde_json::from_slice(input).map_err(|_| CompileError::Structure)?;
            CompiledSemantics::ElementCount(
                array::count(&source, raw).map_err(CompileError::Aggregate)?,
            )
        } else {
            match kind {
                Contract::Boolean
                | Contract::Text
                | Contract::Category
                | Contract::Count
                | Contract::Quantity
                | Contract::Time => {
                    let scalar = if own_frame.is_none()
                        && let Some(frame) = inherited
                    {
                        ScalarContract::compile_vector_coordinate(self.validator, input, frame)
                    } else {
                        ScalarContract::compile(self.validator, input)
                    }
                    .map_err(CompileError::Scalar)?;
                    CompiledSemantics::Scalar(Box::new(scalar))
                }
                Contract::CategoryRange
                | Contract::CountRange
                | Contract::QuantityRange
                | Contract::TimeRange => CompiledSemantics::Range(Box::new(
                    RangeContract::compile(self.validator, input, RangeOptions::default())
                        .map_err(CompileError::Range)?,
                )),
                Contract::Geometry => CompiledSemantics::Geometry(Box::new(
                    GeometryContract::compile(self.validator, input)
                        .map_err(CompileError::Geometry)?,
                )),
                Contract::DataRecord
                | Contract::Vector
                | Contract::DataChoice
                | Contract::DataArray
                | Contract::Matrix => CompiledSemantics::Aggregate(self.aggregate(
                    declaration,
                    kind,
                    &source,
                    &raw,
                )?),
                _ => return Err(CompileError::Structure),
            }
        };
        let encoding = if matches!(kind, Contract::DataArray | Contract::Matrix) {
            raw.get("encoding")
                .map(|raw| EncodingDescriptor::compile(self.validator, raw, None))
                .transpose()?
        } else {
            None
        };
        let index = self.nodes.len();
        self.indices.insert(key, index);
        self.nodes.push(CompiledNode {
            declaration,
            inherited_frame: inherited.map(str::to_owned),
            semantics,
            bindings: Vec::new(),
            encoding,
        });
        // Graph resolution already proved acyclicity/depth/work bounds. Reserve
        // a node before following edges, so repeated references share its entry.
        for offset in 0..self.graph.edges[declaration].len() {
            let edge = &self.graph.edges[declaration][offset];
            let slot = edge.slot;
            let (target, reference) = match edge.target {
                EdgeTarget::Component(target) => (Some(target), None),
                EdgeTarget::Reference(reference) => (
                    match self.graph.references()[reference].target {
                        ReferenceTarget::Local(target) => Some(target),
                        ReferenceTarget::Nonlocal => None,
                    },
                    Some(reference),
                ),
            };
            let name = reference
                .map(|reference| self.graph.references()[reference].name.clone())
                .unwrap_or_else(|| {
                    target.and_then(|target| self.graph.components()[target].name.clone())
                });
            let frame = if kind == Contract::Vector && slot == Slot::Component {
                own_frame.as_deref()
            } else if kind == Contract::Matrix && slot == Slot::ElementType {
                effective_frame
            } else {
                None
            };
            let compiled = match target {
                Some(target) => BindingTarget::Local(self.node(target, frame)?),
                None => BindingTarget::Nonlocal(reference.ok_or(CompileError::Structure)?),
            };
            self.nodes[index].bindings.push(CompiledBinding {
                slot: match slot {
                    Slot::Component => BindingSlot::Component,
                    Slot::ElementType => BindingSlot::ElementType,
                    Slot::ElementCount => BindingSlot::ElementCount,
                    Slot::Selector => BindingSlot::Selector,
                    Slot::Quality => BindingSlot::Quality,
                },
                name,
                target: compiled,
            });
        }
        Ok(index)
    }

    fn aggregate(
        &self,
        declaration: usize,
        kind: Contract,
        source: &Value,
        raw: &BTreeMap<String, &RawValue>,
    ) -> Result<AggregateDescription, CompileError> {
        let error = CompileError::Aggregate;
        if ["value", "quality", "nilValues", "constraint"]
            .iter()
            .any(|member| source.get(member).is_some())
        {
            return Err(error(AggregateError::UnsupportedFeature));
        }
        let is_array = matches!(kind, Contract::DataArray | Contract::Matrix);
        if is_array && source.get("values").is_some() {
            return Err(error(AggregateError::UnsupportedFeature));
        }
        let metadata = aggregate::metadata(source).map_err(error)?;
        let (reference_frame, local_frame) = if matches!(kind, Contract::Vector | Contract::Matrix)
        {
            (
                aggregate::optional_string(source, "referenceFrame").map_err(error)?,
                aggregate::optional_string(source, "localFrame").map_err(error)?,
            )
        } else {
            (None, None)
        };
        if kind == Contract::Vector
            && (reference_frame.is_none()
                || (local_frame.is_some() && local_frame == reference_frame))
        {
            return Err(error(AggregateError::Metadata));
        }
        let children = self.graph.edges[declaration]
            .iter()
            .filter(|edge| matches!(edge.slot, Slot::Component | Slot::ElementType));
        let mut names = BTreeSet::new();
        let mut child_count = 0;
        for edge in children {
            child_count += 1;
            let (name, target) = match edge.target {
                EdgeTarget::Component(target) => (
                    self.graph.components()[target].name.as_deref(),
                    Some(target),
                ),
                EdgeTarget::Reference(reference) => (
                    self.graph.references()[reference].name.as_deref(),
                    match self.graph.references()[reference].target {
                        ReferenceTarget::Local(target) => Some(target),
                        ReferenceTarget::Nonlocal => None,
                    },
                ),
            };
            let name = name
                .filter(|name| aggregate::valid_name(name))
                .ok_or(error(AggregateError::Structure))?;
            if !names.insert(name) {
                return Err(error(AggregateError::DuplicateName));
            }
            if kind == Contract::Vector {
                let target = target.ok_or(error(AggregateError::CoordinateType))?;
                if !matches!(
                    self.graph.components()[target].kind,
                    Contract::Count | Contract::Quantity | Contract::Time
                ) {
                    return Err(error(AggregateError::CoordinateType));
                }
                let child = validation::parse(
                    self.graph
                        .component_source(target)
                        .ok_or(CompileError::Structure)?,
                )
                .map_err(CompileError::Syntax)?;
                if child.get("referenceFrame").is_some() {
                    return Err(error(AggregateError::CoordinateReferenceFrame));
                }
                if child
                    .get("axisID")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
                {
                    return Err(error(AggregateError::CoordinateAxis));
                }
                if child.get("optional").and_then(Value::as_bool) == Some(true) {
                    return Err(error(AggregateError::OptionalCoordinate));
                }
            }
        }
        if kind == Contract::Vector && child_count == 0 {
            return Err(error(AggregateError::EmptyVector));
        }
        if kind == Contract::DataChoice && child_count < 2 {
            return Err(error(AggregateError::ChoiceCardinality));
        }
        let element_count = if is_array {
            let count = source
                .get("elementCount")
                .ok_or(error(AggregateError::MissingElementCount))?;
            let raw = raw
                .get("elementCount")
                .ok_or(error(AggregateError::MissingElementCount))?;
            Some(array::count(count, raw).map_err(error)?)
        } else {
            None
        };
        Ok(AggregateDescription {
            kind,
            metadata,
            reference_frame,
            local_frame,
            element_count,
        })
    }
}

#[cfg(test)]
mod tests;
