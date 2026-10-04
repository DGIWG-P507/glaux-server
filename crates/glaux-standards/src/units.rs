//! Offline, case-sensitive UCUM 2.1 declaration checks, without conversion.
//!
//! The complete pinned terminal dictionary and its notices are in `units/`.
//! General acceptance establishes syntax and known atoms. The separate Time
//! check recognizes a bounded set of sourced temporal atoms. Neither check
//! establishes a property, URI binding, equivalence or conversion. See
//! `docs/ucum.md` for unsupported expressions and resource limits.

use std::{collections::HashMap, sync::OnceLock};

pub const MAX_CODE_BYTES: usize = 4096;
pub const MAX_NESTING: usize = 32;
pub const MAX_COMPONENTS: usize = 256;
pub const MAX_EXPONENT_DIGITS: usize = 6;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnitError {
    Invalid,
    Limit,
    Unsupported,
}

#[derive(Clone, Copy)]
struct Atom {
    metric: bool,
    special: bool,
}

struct Dictionary {
    atoms: HashMap<&'static str, Atom>,
    prefixes: Vec<&'static str>,
}

fn dictionary() -> &'static Dictionary {
    static DICTIONARY: OnceLock<Dictionary> = OnceLock::new();
    DICTIONARY.get_or_init(|| {
        let mut atoms = HashMap::new();
        let mut prefixes = Vec::new();
        // Derived from the complete original. The hosted corpus check verifies
        // every field and original digest, including retained definition values
        // and units required by the upstream licence. No input reaches here.
        for line in include_str!("units/table.tsv").lines() {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 7, "pinned UCUM table record");
            assert_eq!(fields[6], ".", "pinned UCUM table record terminator");
            if fields[0] == "prefix" {
                prefixes.push(fields[1]);
            } else {
                assert!(
                    atoms
                        .insert(
                            fields[1],
                            Atom {
                                metric: fields[2] == "yes",
                                special: fields[3] == "yes",
                            },
                        )
                        .is_none(),
                    "unique pinned UCUM atom"
                );
            }
        }
        prefixes.sort_by_key(|prefix| std::cmp::Reverse(prefix.len()));
        Dictionary { atoms, prefixes }
    })
}

/// Validate a declaration against the incorporated, case-sensitive UCUM 2.1
/// basis. No URI, file or network lookup is performed and no value is converted.
///
/// Whitespace is rejected, including inside annotations. The original string
/// is neither normalized nor rewritten. Bounds are implementation limits, not
/// claims that otherwise valid longer UCUM expressions are malformed.
pub fn validate_code(code: &str) -> Result<(), UnitError> {
    if code.len() > MAX_CODE_BYTES {
        return Err(UnitError::Limit);
    }
    if code.is_empty() || !code.bytes().all(|byte| (33..=126).contains(&byte)) {
        return Err(UnitError::Invalid);
    }
    let mut parser = Parser {
        code,
        pos: 0,
        components: 0,
        has_special: false,
        has_operator: false,
        unsupported_power: false,
    };
    if parser.peek() == Some(b'/') {
        parser.pos += 1;
        parser.has_operator = true;
    }
    parser.term(0)?;
    if parser.pos != code.len() {
        return Err(UnitError::Invalid);
    }
    if parser.has_special && (parser.has_operator || parser.unsupported_power) {
        return Err(UnitError::Unsupported);
    }
    Ok(())
}

/// Validate a temporal unit declaration without converting a numeric Time value.
///
/// The supported atoms are UCUM 2.1's thirteen entries whose property is `time`,
/// and the pinned prefixes applied to seconds. Parentheses, inert annotations
/// and a power of one are allowed. Other compound expressions and dimensional
/// derivations return `Unsupported`; known incompatible simple units return
/// `Invalid`. Calendar arithmetic, origin and reference-frame binding belong to
/// the Time component, not this declaration check.
pub fn validate_time_code(code: &str) -> Result<(), UnitError> {
    validate_code(code)?;
    // This is a temporary classification view. Neither the submitted code nor
    // its unit declaration is rewritten. The preceding parser supplies bounds
    // and verifies the complete syntax before any simplification.
    let plain = without_annotations(code);
    let mut simple = plain.as_str();
    while let Some(inner) = simple
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
    {
        simple = inner;
    }
    let (symbol, power) = split_simple_power(simple);
    if symbol.bytes().all(|byte| byte.is_ascii_digit()) {
        // Also covers annotation-only unity after annotations are removed.
        return Err(UnitError::Invalid);
    }
    if known_atom(symbol).is_err() {
        // The whole input already passed syntax/dictionary validation. A
        // non-atom here is therefore an expression requiring further analysis.
        return Err(UnitError::Unsupported);
    }
    if symbol == "[S]" {
        // The Svedberg definition has a time dimension but its property is
        // sedimentation coefficient. Do not infer a Time quantity from that.
        return Err(UnitError::Unsupported);
    }
    let temporal = is_temporal_atom(symbol);
    let power_one = power.is_empty()
        || (!power.starts_with('-') && power.trim_start_matches(['+', '0']) == "1");
    match (temporal, power_one) {
        (true, true) => Ok(()),
        (true, false) | (false, true) => Err(UnitError::Invalid),
        // For example Hz-1 is temporal, but establishing this involves a
        // dimensional derivation outside the explicitly supported atom set.
        (false, false) => Err(UnitError::Unsupported),
    }
}

fn is_temporal_atom(symbol: &str) -> bool {
    // Complete set of property=time entries in the pinned 2.1 essence. Years
    // and months denote its specified mean durations, not calendar arithmetic.
    matches!(
        symbol,
        "s" | "min"
            | "h"
            | "d"
            | "wk"
            | "a_t"
            | "a_j"
            | "a_g"
            | "a"
            | "mo_s"
            | "mo_j"
            | "mo_g"
            | "mo"
    ) || symbol
        .strip_suffix('s')
        .is_some_and(|prefix| dictionary().prefixes.contains(&prefix))
}

fn without_annotations(code: &str) -> String {
    let mut plain = String::with_capacity(code.len());
    let mut annotation = false;
    let mut bracket = false;
    for byte in code.bytes() {
        if annotation {
            if byte == b'}' {
                annotation = false;
            }
        } else if byte == b'{' && !bracket {
            annotation = true;
        } else {
            if byte == b'[' {
                bracket = true;
            } else if byte == b']' {
                bracket = false;
            }
            plain.push(char::from(byte));
        }
    }
    plain
}

fn split_simple_power(code: &str) -> (&str, &str) {
    let digits_start = code.trim_end_matches(|ch: char| ch.is_ascii_digit()).len();
    if digits_start == 0 || digits_start == code.len() {
        return (code, "");
    }
    let power_start = if matches!(code.as_bytes()[digits_start - 1], b'+' | b'-') {
        digits_start - 1
    } else {
        digits_start
    };
    (&code[..power_start], &code[power_start..])
}

struct Parser<'a> {
    code: &'a str,
    pos: usize,
    components: usize,
    has_special: bool,
    has_operator: bool,
    unsupported_power: bool,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.code.as_bytes().get(self.pos).copied()
    }

    fn term(&mut self, depth: usize) -> Result<(), UnitError> {
        self.component(depth)?;
        while matches!(self.peek(), Some(b'.' | b'/')) {
            self.pos += 1;
            self.has_operator = true;
            self.component(depth)?;
        }
        Ok(())
    }

    fn component(&mut self, depth: usize) -> Result<(), UnitError> {
        self.components += 1;
        if self.components > MAX_COMPONENTS {
            return Err(UnitError::Limit);
        }
        match self.peek() {
            Some(b'(') => {
                if depth >= MAX_NESTING {
                    return Err(UnitError::Limit);
                }
                self.pos += 1;
                self.term(depth + 1)?;
                if self.peek() != Some(b')') {
                    return Err(UnitError::Invalid);
                }
                self.pos += 1;
                // UCUM 2.1 §10 removed powers on parenthesized expressions.
                Ok(())
            }
            Some(b'{') => self.annotation(),
            _ => self.simple(),
        }
    }

    fn simple(&mut self) -> Result<(), UnitError> {
        let start = self.pos;
        let mut bracket = false;
        let mut last_atom_end = start;
        while let Some(byte) = self.peek() {
            if bracket {
                match byte {
                    b'[' => return Err(UnitError::Invalid),
                    b']' => bracket = false,
                    _ => {}
                }
                self.pos += 1;
                last_atom_end = self.pos;
            } else {
                match byte {
                    b'[' => {
                        bracket = true;
                        self.pos += 1;
                        last_atom_end = self.pos;
                    }
                    b'"' | b'(' | b')' | b'+' | b'-' | b'.' | b'/' | b'=' | b']' | b'{' | b'}' => {
                        break;
                    }
                    _ => {
                        self.pos += 1;
                        if !byte.is_ascii_digit() {
                            last_atom_end = self.pos;
                        }
                    }
                }
            }
        }
        if bracket || self.pos == start {
            return Err(UnitError::Invalid);
        }
        let numeric = last_atom_end == start;
        let special = if numeric {
            // §8 requires a positive integer factor. Avoid bounded-machine
            // numeric parsing: the declaration's overall byte bound suffices.
            if self.code[start..self.pos].bytes().all(|byte| byte == b'0') {
                return Err(UnitError::Invalid);
            }
            false
        } else {
            let special = known_atom(&self.code[start..last_atom_end])?.special;
            self.has_special |= special;
            special
        };

        let unsigned_exponent = !numeric && last_atom_end < self.pos;
        if unsigned_exponent {
            self.unsupported_power |= Self::exponent(&self.code[last_atom_end..self.pos], special)?;
        }
        if matches!(self.peek(), Some(b'+' | b'-')) {
            if unsigned_exponent {
                return Err(UnitError::Invalid);
            }
            // §9's explicit 2+10 example permits a signed exponent on a
            // positive integer factor despite its omission from the BNF.
            let exponent_start = self.pos;
            self.pos += 1;
            let digits_start = self.pos;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.pos += 1;
            }
            if digits_start == self.pos {
                return Err(UnitError::Invalid);
            }
            self.unsupported_power |=
                Self::exponent(&self.code[exponent_start..self.pos], special)?;
        }
        if self.peek() == Some(b'{') {
            // §8 allows positive integer factors in place of simple symbols;
            // §6 annotations end the symbol without changing unit semantics.
            self.annotation()?;
        }
        Ok(())
    }

    fn exponent(exponent: &str, special: bool) -> Result<bool, UnitError> {
        let digits = exponent.trim_start_matches(['+', '-']);
        if digits.len() > MAX_EXPONENT_DIGITS {
            return Err(UnitError::Limit);
        }
        Ok(special && (exponent.starts_with('-') || digits.trim_start_matches('0') != "1"))
    }

    fn annotation(&mut self) -> Result<(), UnitError> {
        self.pos += 1;
        while let Some(byte) = self.peek() {
            self.pos += 1;
            match byte {
                b'{' => return Err(UnitError::Invalid),
                b'}' => return Ok(()),
                _ => {}
            }
        }
        Err(UnitError::Invalid)
    }
}

fn known_atom(symbol: &str) -> Result<Atom, UnitError> {
    let dictionary = dictionary();
    // §4: longest prefix leaving a metric atom first; only then the bare atom.
    // A prefix cannot be stacked on an already prefixed expression.
    for prefix in &dictionary.prefixes {
        if let Some(atom) = symbol
            .strip_prefix(*prefix)
            .and_then(|rest| dictionary.atoms.get(rest))
            .filter(|atom| atom.metric)
        {
            return Ok(*atom);
        }
    }
    dictionary
        .atoms
        .get(symbol)
        .copied()
        .ok_or(UnitError::Invalid)
}

#[cfg(test)]
mod tests;
