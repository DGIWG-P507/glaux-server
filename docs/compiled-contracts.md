# Immutable SWE compiled contracts

[Task #38 / 2.1.12](https://github.com/DGIWG-P507/glaux-server/issues/38)
assembles the existing component checks into one owned description. To keep a
measurement's meaning stable, the compiler retains its exact source and builds
read-only semantic plans. Compiling another description cannot change the first.
This is the shared model foundation, not stream registration or a wire codec.

## Source, meaning and bounds

`glaux_standards::compiled::CompiledContract::compile` takes a fixed descriptor
kind, bounded JSON bytes, source media-type metadata and explicit array-schema
options. It owns the original bytes and media type. `sha256()` is the 32-byte
SHA-256 of those exact bytes, including whitespace; it is not a hash of parsed
or re-serialized JSON. The digest is an integrity identifier, not a signature
or proof of origin. Media-type metadata is retained separately from the payload
format selected inside a CSAPI wrapper; this library is not HTTP negotiation.

The graph preserves declared order, IDs and source locations. Compiled entries
reuse the existing scalar, range, geometry, quality and aggregate rules. Local
references point to indexed entries rather than expanding a shared graph into
copies of JSON. A component used under different inherited frames has distinct
checked contexts. The original declaration is never rewritten to add a frame.
Metadata-only nonlocal references remain explicit and are never fetched;
their presence is not a claim that a payload can be executed.

Whole-input syntax and graph limits still apply. Compilation also bounds the
total semantic entries (4,096) and cumulative source lengths charged for plan
roots and semantic contexts (8 MiB). Helpers may inspect those bytes more than
once; this is not an exact CPU-work measurement. These are Glaux budgets,
not standard-imposed sizes.
Errors return no partially usable contract, even when earlier roots
compiled successfully. Public access is read-only; callers cannot mutate the
retained input through their original buffer or a shared plan registry.

## Enclosing descriptors

Fixed entry points cover a component document, ordinary-JSON observation and
command schema wrappers, and SWE observation and command schema wrappers.
Every supplied known component-root member is compiled, not just the first.
The complete original wrapper is checked before assembling its contents.
Ordinary wrappers retain their optional result-link metadata without fetching
it; a `resultSchema` is SWE structure, not a supplied JSON Schema program.

SWE wrappers preserve the exact encoding descriptor and validate it against
the selected JSON, Text or Binary definition and the enclosing payload format.
The CSV wrapper's delimiter restrictions remain enforced. An embedded array
encoding is also a checked description, never acceptance of an inline payload.
Descriptor validation
does not execute framing, resolve Binary member paths, prove interoperability
or advertise an implemented encoding class. Those are later codec tasks.
Existing schema corrections are opt-in and retain their qualified source
verdict; this task does not edit the standards corpus.

## Verification and limits

Source-attributed tests cover every implemented component family, declared
ordering and metadata, local bindings, quality checks, inherited frames,
wrappers and bounds. Expected bytes, fields and digest are independently stated.
They compile A and then B, mutate/release caller-owned inputs, and check A again.
A disposable fault skips a later invalid wrapper root; the atomic-publication
test must detect that mistake after its fresh unmodified baseline passes.
The existing Rust CI lane checks discovery, execution and the fault's exact
semantic failure; setup or compilation failures cannot count as detection.

Controlling sources are [Guide §§4.3, 6.1 and 8.1.1](https://github.com/DGIWG-P507/glaux/blob/0ad4504c75ec13c7ba530a039c7f80d9273380c1/Docs/Plans/glaux-server/glaux-server-implementation-guide.md),
the [unchanged pinned corpus](standards-corpus.md), and the component documents
linked from the README. [Digest dependency evidence](dependencies.md) records
the reused locked RustCrypto package. The issue/PR records actual hosted checks,
failures, tested commit and separate review. No new endpoint, persisted stream,
historical-value store, codec execution or conformance declaration is added.
