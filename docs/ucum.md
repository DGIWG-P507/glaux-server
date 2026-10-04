# UCUM code declarations

Task 2.1.2 validates Quantity `uom.code` with the case-sensitive UCUM **2.1**
basis incorporated by SWE Common 3.0. This follows Implementation Guide §4.3
and accepted IDR-SRV-024 §7.1. Task 2.1.3 adds the bounded temporal-unit check
described below for numeric Time declarations. The general code check preserves
the submitted text and does not convert values, infer dimensions or property identity, compare
equivalent expressions, resolve a semantic URI or establish code/URI agreement.
It is not a claim of full UCUM semantic conformance or complete SWE conformance.

The separate UnitReference model retains `code`, `href`, `label` and `symbol`.
A valid absolute semantic URI remains explicitly unresolved; it is not passed
to the UCUM parser or fetched. Supplying an `href` does not excuse an invalid
`code`. Neither labels nor display symbols establish unit identity.

## Incorporated source

Both originals are unchanged from official `ucum-org/ucum` release-2.1 commit
[`910f502003269a492e32dd5cb96e5503b2351ac9`](https://github.com/ucum-org/ucum/tree/910f502003269a492e32dd5cb96e5503b2351ac9).
UCUM 2.2 and mutable online vocabularies are not substituted for that basis.

| Original | SHA-256 |
| --- | --- |
| [ucum-essence.xml](../crates/glaux-standards/src/units/originals/ucum-essence.xml) | `774b99e5ec13c6d9f3bc875bb91f86f5a4da08020c645adf812039883f4beb8c` |
| [ucum-source.xml](../crates/glaux-standards/src/units/originals/ucum-source.xml) | `78974be02ec3dc44985f4833a30323d22bf18debae2ff11c222b71b51e27f6fa` |

The complete essence includes 24 prefixes, 7 base units and 303 defined atoms.
The derived [lookup table](../crates/glaux-standards/src/units/table.tsv) contains
every entry, in source order. Fields are kind, case-sensitive code, metric flag,
special flag, original definition value, original definition unit and a record
terminator. Empty source fields remain empty. All base units are metric. A
source `isSpecial` flag or definition function marks a special scale: some
retired homeopathic units carry the function without the flag. There are 21
such entries. Definition values and units are retained for provenance and the
upstream licence, and are never evaluated by this validator.

The original specification includes its contemporaneous UCUM copyright and
licence. The [full text](../crates/glaux-standards/src/units/license.txt) and
[required short notice](../crates/glaux-standards/src/units/UCUM_short_license.txt)
are also retained beside the table, with their original wording and historical
URLs. These materials are separately licensed; Glaux's Apache-2.0 licence does
not relicense them. Keep the source, version and notices with distributions
containing the table, and expose the notices alongside any downloadable product
containing it, as its licence requires. No dependency or installation is added.

## Accepted expressions and explicit limits

The validator uses UCUM 2.1 §§3–12: known atoms, the longest prefix leaving a
metric atom, integer factors, signed or unsigned integer powers, dot products,
solidus division, parentheses, and annotations. Bracket contents belong to the
atom, including punctuation in `[m/s2/Hz^(1/2)]`. Prefixes cannot be stacked,
applied to nonmetric atoms or put in front of parentheses. Case and spelling
are significant: `kg`, `Ohm` and `Cel` work; `KG`, `ohm` and `CEL` do not.

Whitespace and non-ASCII characters are rejected, including inside annotations.
Annotations are accepted as semantically inert text, not dictionary extensions.
`{RBC}` therefore denotes annotated unity and does not prove a red-cell unit or
property binding. Numeric factors must be positive. A dot is multiplication,
so `2.5` is a valid unit expression for `2 × 5`, not the decimal 2.5. Section 9's
explicit `2+10` example is accepted even though its BNF omits the corresponding
factor exponent production; §8's substitution of a positive integer for a
simple symbol likewise permits `1{ratio}`. Parenthesized powers, removed in
§10 before 2.1, are rejected. The BNF permits a leading solidus only at the
top level, so `/s` works and `(/s)` is rejected.

Non-ratio special scales such as `Cel`, `mCel`, `[degF]`, `dB`, `[pH]` and
`[m/s2/Hz^(1/2)]` are recognized, including parenthesized or annotated forms
and an explicit power of one. Any algebraic expression containing a special
scale or a different power on it returns `UnitError::Unsupported`. This includes
scalar scaling such as `2.Cel`, even though UCUM §22 defines that operation;
this declaration checker does not implement the semantic algebra needed to
distinguish permitted scaling from invalid combinations. Ordinary and arbitrary
units can form expressions such as `kg.m/s2` and `[IU]/mL`; acceptance does not
make arbitrary units commensurable.

`UnitError::Invalid` identifies malformed syntax, unknown atoms, invalid prefix
combinations and nonpositive factors. `UnitError::Limit` separately identifies
more than 4096 input bytes, 32 parenthesis levels, 256 components (including
parenthesized components), or 6 exponent digits. Integer factors need no machine
integer conversion and are bounded by the input limit. These bounds are local
resource limits, not amendments to UCUM. No runtime file or network access is
performed.

## Temporal declarations

`validate_time_code` first applies the complete generic code check, then confirms
that the declaration belongs to its explicitly supported temporal subset. It
accepts the complete set of thirteen UCUM 2.1 atoms whose original `property` is
`time`: `s`, `min`, `h`, `d`, `wk`, `a_t`, `a_j`, `a_g`, `a`, `mo_s`, `mo_j`,
`mo_g` and `mo`. All 24 pinned UCUM prefixes can be applied to the metric second,
including `ms`, `us`, `ns` and `Kis`. Parentheses, inert annotations and an explicit
power of one are accepted without changing the source declaration.

The incorporated time table defines years and months as specified mean durations:
for example `a` refers to `a_j`, and `mo` to `mo_j`. Recognizing these codes does
not implement variable calendar-month arithmetic or conversion to seconds.
The numeric value, frame and origin remain the Time component's responsibility;
this helper establishes no UTC instant, leap-second handling or frame conversion.

Known incompatible simple units such as `m`, `kg`, `Hz`, `1` and `Cel` return
`Invalid`; so do powers other than one on temporal atoms, such as `s2` and `s-1`.
Compounds such as `1.s`, `s.m/m` and `1/Hz`, and other dimensional derivations
such as `Hz-1`, return `Unsupported` even where a full analysis could establish
a time dimension. The Svedberg atom `[S]` also returns `Unsupported`: its
definition has a time dimension, but its declared property is sedimentation
coefficient. No property equivalence is inferred from matching dimensions.
Malformed syntax and the existing resource limits keep their `Invalid` and
`Limit` results. A generic UCUM pass alone is never accepted as proof of a
temporal declaration.

## Verification

The hosted static lane runs the standard-library-only check:

```sh
python3 crates/glaux-standards/src/units/verify.py --self-test
```

It verifies both original digests and the file inventory, re-derives every table
field and both notices, and compares exact checked-in bytes. Its self-test
changes each of the five checked assets in memory and requires rejection.
`--emit-table` reproduces the complete lookup on stdout after verification;
neither command downloads or overwrites a source artifact. The historical
specification's DTD is not evaluated; licence extraction uses its digest-checked
text. The essence has no external dependencies.

Rust tests separately check hand-selected source-derived positive and negative
examples, every atom and metric prefix combination, special-scale limitations
and resource boundaries. Table-loop acceptance alone is not the source oracle:
the independent original-byte and re-derivation check establishes which complete
dictionary was used. Execution evidence belongs to the task PR and issue;
having these checks in the checkout does not imply that they have run.

`ucum_time_codes_use_temporal_atoms` independently names all thirteen sourced
temporal atoms and 24 prefixes, with annotated and parenthesized examples.
`ucum_time_codes_reject_or_defer_other_units` separately checks incompatible
units, valid but unsupported expressions, malformed input and inherited limits.
