//! Distinct local locators, published UIDs and authority-qualified source IDs.
//!
//! None of these types is a permission, trusted issuer or observation time.
//! UUIDv7 leaks approximate minting time; "opaque" does not mean confidential.
//! Persistence owns uniqueness, conflicts and non-reuse after deletion.

use std::{fmt, str::FromStr, time::{Duration, SystemTime, UNIX_EPOCH}};

/// Local parsing budget, not an OGC or URI-standard length limit.
pub const MAX_IDENTITY_TEXT_BYTES: usize = 4096;
const MAX_MILLIS: u128 = (1_u128 << 48) - 1;

/// Safe diagnostics deliberately contain none of the supplied identity text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityError {
    InvalidLocalId,
    InvalidUid,
    InvalidSourceText,
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidLocalId => "expected a canonical lowercase UUIDv7 local ID",
            Self::InvalidUid => "expected an absolute URI within the identity text budget",
            Self::InvalidSourceText => "expected nonempty source text without controls within the identity text budget",
        })
    }
}

impl std::error::Error for IdentityError {}

/// Generation fails explicitly rather than substituting a clock or weak entropy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenerationError {
    ClockBeforeEpoch,
    ClockOutOfRange,
    EntropyUnavailable,
}

impl fmt::Display for GenerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ClockBeforeEpoch => "local ID clock precedes the Unix epoch",
            Self::ClockOutOfRange => "local ID clock exceeds the UUIDv7 timestamp range",
            Self::EntropyUnavailable => "local ID operating-system entropy is unavailable",
        })
    }
}

impl std::error::Error for GenerationError {}

/// An opaque locator with a strict Glaux text contract, not every UUID spelling.
/// No timestamp, authorization, raw UUID or implicit cross-identity conversion
/// is exposed. UUID bytes themselves are not secret or proof of uniqueness.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LocalId(uuid::Uuid);

impl LocalId {
    /// Mint with the current Unix millisecond and fresh OS randomness.
    ///
    /// Not guaranteed monotonic or collision-free; storage must reject conflicts.
    /// OS entropy can block (for example during early boot); no deadline is promised.
    pub fn generate() -> Result<Self, GenerationError> {
        let elapsed = SystemTime::now().duration_since(UNIX_EPOCH)
            .map_err(|_| GenerationError::ClockBeforeEpoch);
        Self::generate_with(elapsed, |bytes| {
            getrandom::fill(bytes).map_err(|_| GenerationError::EntropyUnavailable)
        })
    }

    // Private deterministic seam: never offer callers insecure entropy injection.
    fn generate_with(
        elapsed: Result<Duration, GenerationError>,
        fill: impl FnOnce(&mut [u8; 10]) -> Result<(), GenerationError>,
    ) -> Result<Self, GenerationError> {
        let millis = elapsed?.as_millis();
        if millis > MAX_MILLIS {
            return Err(GenerationError::ClockOutOfRange);
        }
        let mut entropy = [0_u8; 10];
        fill(&mut entropy)?; // On error even partially filled bytes must be discarded.
        Ok(Self(uuid::Builder::from_unix_timestamp_millis(millis as u64, &entropy).into_uuid()))
    }
}

impl FromStr for LocalId {
    type Err = IdentityError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.len() != 36 || !text.bytes().enumerate().all(|(i, byte)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        }) {
            return Err(IdentityError::InvalidLocalId);
        }
        let value = uuid::Uuid::try_parse(text).map_err(|_| IdentityError::InvalidLocalId)?;
        let bytes = value.as_bytes();
        if bytes[6] >> 4 != 7 || bytes[8] & 0xc0 != 0x80 {
            return Err(IdentityError::InvalidLocalId);
        }
        Ok(Self(value))
    }
}

impl fmt::Display for LocalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A published absolute RFC 3986 URI, preserved byte-for-byte, not normalized.
/// Parsing establishes syntax only, never existence, ownership or permission.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Uid(String);

impl Uid {
    pub fn as_str(&self) -> &str { &self.0 }
}

impl FromStr for Uid {
    type Err = IdentityError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.len() > MAX_IDENTITY_TEXT_BYTES || fluent_uri::Uri::parse(text).is_err() {
            return Err(IdentityError::InvalidUid);
        }
        Ok(Self(text.to_owned()))
    }
}

impl fmt::Display for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
}

fn source_text(text: &str) -> Result<String, IdentityError> {
    if text.is_empty() || text.len() > MAX_IDENTITY_TEXT_BYTES || text.chars().any(char::is_control) {
        return Err(IdentityError::InvalidSourceText);
    }
    Ok(text.to_owned())
}

/// Opaque source namespace/context, not necessarily a URI or a trusted issuer.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SourceAuthority(String);

impl SourceAuthority {
    pub fn as_str(&self) -> &str { &self.0 }
}

impl FromStr for SourceAuthority {
    type Err = IdentityError;
    fn from_str(text: &str) -> Result<Self, Self::Err> { source_text(text).map(Self) }
}

impl fmt::Display for SourceAuthority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
}

/// Opaque source-assigned value; meaningful as an identity only with its authority.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SourceIdentifier(String);

impl SourceIdentifier {
    pub fn as_str(&self) -> &str { &self.0 }
}

impl FromStr for SourceIdentifier {
    type Err = IdentityError;
    fn from_str(text: &str) -> Result<Self, Self::Err> { source_text(text).map(Self) }
}

impl fmt::Display for SourceIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
}

/// Equality and hashing use both source fields; no inference of a local locator.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SourceIdentity {
    authority: SourceAuthority,
    identifier: SourceIdentifier,
}

impl SourceIdentity {
    pub fn new(authority: SourceAuthority, identifier: SourceIdentifier) -> Self {
        Self { authority, identifier }
    }
    pub fn authority(&self) -> &SourceAuthority { &self.authority }
    pub fn identifier(&self) -> &SourceIdentifier { &self.identifier }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // Independently published RFC 9562 Appendix A.6, not output of our generator.
    const RFC_ID: &str = "017f22e2-79b0-7cc3-98c4-dc0c0c07398f";

    #[test]
    fn local_id_rfc_vector_and_exact_round_trip() {
        let id: LocalId = RFC_ID.parse().unwrap();
        assert_eq!(id.to_string(), RFC_ID);
        assert_eq!(id.0.as_bytes(), &[0x01,0x7f,0x22,0xe2,0x79,0xb0,0x7c,0xc3,0x98,0xc4,0xdc,0x0c,0x0c,0x07,0x39,0x8f]);
        let minted = LocalId::generate_with(Ok(Duration::from_millis(1645557742000)), |bytes| {
            *bytes = [0x0c,0xc3,0x18,0xc4,0xdc,0x0c,0x0c,0x07,0x39,0x8f];
            Ok(())
        }).unwrap();
        assert_eq!(minted.to_string(), RFC_ID);
    }

    #[test]
    fn local_id_rejects_malformed_and_noncanonical_forms() {
        for text in ["", "sensor-12", "017f22e279b07cc398c4dc0c0c07398f",
            "017F22E2-79B0-7CC3-98C4-DC0C0C07398F", "017f22e2-79b0-7cc3-98c4-dc0c0c07398g",
            "{017f22e2-79b0-7cc3-98c4-dc0c0c07398f}", "urn:uuid:017f22e2-79b0-7cc3-98c4-dc0c0c07398f",
            " 017f22e2-79b0-7cc3-98c4-dc0c0c07398f", "017f22e2-79b0-7cc3-98c4-dc0c0c07398f\n",
            "017f22e2%2d79b0-7cc3-98c4-dc0c0c07398f", "017f22e2_79b0-7cc3-98c4-dc0c0c07398f",
            "00000000-0000-0000-0000-000000000000", "ffffffff-ffff-ffff-ffff-ffffffffffff"] {
            assert_eq!(text.parse::<LocalId>(), Err(IdentityError::InvalidLocalId), "{text:?}");
        }
    }

    #[test]
    fn local_id_rejects_every_wrong_version_and_variant() {
        // Cartesian set, expectations specified from version=7 and RFC variant=10xx.
        for version in 0..16 {
            for variant in 0..16 {
                let text = format!("017f22e2-79b0-{version:x}cc3-{variant:x}8c4-dc0c0c07398f");
                let result = text.parse::<LocalId>();
                assert_eq!(result.is_ok(), version == 7 && (8..=11).contains(&variant), "{text}");
                if let Ok(id) = result { assert_eq!(id.to_string(), text); }
            }
        }
    }

    #[test]
    fn generation_checks_clock_and_entropy_before_emitting_id() {
        assert_eq!(LocalId::generate_with(Err(GenerationError::ClockBeforeEpoch), |_| panic!("entropy on invalid clock")), Err(GenerationError::ClockBeforeEpoch));
        assert_eq!(LocalId::generate_with(Ok(Duration::from_millis(1_u64 << 48)), |_| panic!("entropy on overflowing clock")), Err(GenerationError::ClockOutOfRange));
        assert_eq!(LocalId::generate_with(Ok(Duration::ZERO), |bytes| {
            bytes.fill(0x55); // Partial or complete writes must not escape after an error.
            Err(GenerationError::EntropyUnavailable)
        }), Err(GenerationError::EntropyUnavailable));
        let first = LocalId::generate_with(Ok(Duration::ZERO), |bytes| { bytes.fill(0); Ok(()) }).unwrap();
        assert_eq!(first.to_string(), "00000000-0000-7000-8000-000000000000");
        let last = LocalId::generate_with(Ok(Duration::from_millis((1_u64 << 48)-1)), |bytes| { bytes.fill(0xff); Ok(()) }).unwrap();
        assert_eq!(last.to_string(), "ffffffff-ffff-7fff-bfff-ffffffffffff");
    }

    #[test]
    fn generation_uses_fresh_randomness_without_claiming_temporal_order() {
        let make = |millis, entropy| LocalId::generate_with(Ok(Duration::from_millis(millis)), |bytes| { bytes.fill(entropy); Ok(()) }).unwrap();
        // Repeated/rolled-back clocks are allowed. The millisecond is not domain time.
        let ids = [make(123, 0), make(123, 1), make(122, 2)];
        assert_eq!(ids.into_iter().collect::<HashSet<_>>().len(), 3);
        for _ in 0..64 {
            let id = LocalId::generate().expect("approved runner OS entropy and clock");
            let text = id.to_string();
            assert_eq!(text.as_bytes()[14], b'7');
            assert!(matches!(text.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
            assert_eq!(text.parse::<LocalId>().unwrap(), id);
        }
    }

    #[test]
    fn uid_preserves_absolute_uri_spelling_without_local_id_inference() {
        for text in ["urn:example:temperature", "https://EXAMPLE.test/a%2Fb?x=%41#part", "custom:value", "x:", "file:///sensor"] {
            let uid: Uid = text.parse().unwrap();
            assert_eq!(uid.as_str(), text);
            assert_eq!(uid.to_string(), text);
            assert!(text.parse::<LocalId>().is_err());
        }
        assert_ne!("https://example.test/%41".parse::<Uid>().unwrap(), "https://example.test/A".parse::<Uid>().unwrap());
        assert_ne!("https://EXAMPLE.test/a".parse::<Uid>().unwrap(), "https://example.test/a".parse::<Uid>().unwrap());
        let full = format!("urn:uuid:{RFC_ID}");
        assert!(full.parse::<Uid>().is_ok());
        assert!(full.parse::<LocalId>().is_err());
        for text in ["", RFC_ID, "sensor-12", "/relative", "//host/path", "x:a b", " x:a", "x:a\n", "x:%zz", "x:%", "x:é", "1x:value"] {
            assert_eq!(text.parse::<Uid>(), Err(IdentityError::InvalidUid), "{text:?}");
        }
        let at_limit = format!("x:{}", "a".repeat(MAX_IDENTITY_TEXT_BYTES-2));
        assert!(at_limit.parse::<Uid>().is_ok());
        assert!(format!("{at_limit}a").parse::<Uid>().is_err());
    }

    #[test]
    fn source_identity_requires_both_fields_and_preserves_text() {
        let identifier: SourceIdentifier = " device:α%2f ".parse().unwrap();
        let first = SourceIdentity::new("Sensor Fleet A".parse().unwrap(), identifier.clone());
        let second = SourceIdentity::new("Sensor Fleet B".parse().unwrap(), identifier);
        assert_ne!(first, second);
        assert_eq!(first.authority().as_str(), "Sensor Fleet A");
        assert_eq!(first.identifier().as_str(), " device:α%2f ");
        assert_eq!([first.clone(), first, second].into_iter().collect::<HashSet<_>>().len(), 2);
        // A source can use URI- or UUID-looking text without changing its type/meaning.
        assert_eq!(RFC_ID.parse::<SourceIdentifier>().unwrap().as_str(), RFC_ID);
        assert_eq!("urn:example:source".parse::<SourceAuthority>().unwrap().as_str(), "urn:example:source");
        for text in ["", "a\n", "\0", "a\u{0085}"] {
            assert_eq!(text.parse::<SourceAuthority>(), Err(IdentityError::InvalidSourceText));
            assert_eq!(text.parse::<SourceIdentifier>(), Err(IdentityError::InvalidSourceText));
        }
        let limit = "é".repeat(MAX_IDENTITY_TEXT_BYTES/2);
        assert!(limit.parse::<SourceAuthority>().is_ok());
        assert!(limit.parse::<SourceIdentifier>().is_ok());
        let too_long = format!("{limit}a");
        assert!(too_long.parse::<SourceAuthority>().is_err());
        assert!(too_long.parse::<SourceIdentifier>().is_err());
    }
}
