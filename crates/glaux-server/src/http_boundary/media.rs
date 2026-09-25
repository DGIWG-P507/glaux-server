//! Bounded RFC 9110 media/coding parsing; no body decoder or resource registry.
use super::Problem;
use axum::http::{HeaderMap, HeaderName, header};

const MAX_BYTES: usize = 16_384;
const MAX_RANGES: usize = 128;
const MAX_PARAMETERS: usize = 16;
const MAX_EMPTY_ELEMENTS: usize = 128;

#[derive(Debug)]
struct Media {
    kind: Vec<u8>,
    subtype: Vec<u8>,
    parameters: Vec<(Vec<u8>, Vec<u8>)>,
    quality: u16,
}

pub(super) fn negotiate(headers: &HeaderMap, offered: &[&str]) -> Result<usize, Problem> {
    if offered.is_empty() || offered.len() > MAX_RANGES {
        return Err(Problem::internal());
    }
    // Check the complete server offer even when Accept is absent: malformed
    // server configuration is not client input and must not become a 400.
    let representations = offered
        .iter()
        .map(|value| parse_media(value.as_bytes(), false).map_err(|()| Problem::internal()))
        .collect::<Result<Vec<_>, _>>()?;
    let Some(accept) = combined(headers, header::ACCEPT)? else {
        return Ok(0);
    };
    let ranges = list(&accept)
        .map_err(|()| Problem::bad_request())?
        .into_iter()
        .map(|value| parse_media(value, true).map_err(|()| Problem::bad_request()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut selected: Option<(usize, (u16, u8, usize))> = None;
    for (index, representation) in representations.iter().enumerate() {
        // Specificity determines this representation's quality before quality
        // is used to compare representations. An explicit q=0 wins over */*.
        let mut matched: Option<((u8, usize), u16)> = None;
        for range in &ranges {
            if let Some(specificity) = matches(range, representation)
                && matched.is_none_or(|(prior, quality)| {
                    specificity > prior || (specificity == prior && range.quality > quality)
                })
            {
                matched = Some((specificity, range.quality));
            }
        }
        if let Some(((kind, parameters), quality)) = matched {
            let rank = (quality, kind, parameters);
            if quality > 0 && selected.is_none_or(|(_, prior)| rank > prior) {
                selected = Some((index, rank));
            }
        }
    }
    selected
        .map(|(index, _)| index)
        .ok_or_else(Problem::not_acceptable)
}

pub(super) fn check_json_media(headers: &HeaderMap) -> Result<(), Problem> {
    let values = headers.get_all(header::CONTENT_TYPE);
    let mut fields = values.iter();
    let Some(value) = fields.next() else {
        return Err(Problem::unsupported_media_type(false));
    };
    if fields.next().is_some() {
        return Err(Problem::bad_request());
    }
    let media = parse_media(value.as_bytes(), false).map_err(|()| Problem::bad_request())?;
    if media.kind == b"application" && media.subtype == b"json" {
        // RFC 8259's application/json registration defines no parameters that
        // change JSON decoding. They are syntax-checked, not schema selectors.
        Ok(())
    } else {
        Err(Problem::unsupported_media_type(false))
    }
}

pub(super) fn check_coding(headers: &HeaderMap) -> Result<(), Problem> {
    let Some(codings) = combined(headers, header::CONTENT_ENCODING)? else {
        return Ok(());
    };
    let codings = list(&codings).map_err(|()| Problem::bad_request())?;
    let mut unsupported = false;
    for coding in codings {
        if !coding.iter().copied().all(token_byte) {
            return Err(Problem::bad_request());
        }
        unsupported |= !coding.eq_ignore_ascii_case(b"identity");
    }
    if unsupported {
        Err(Problem::unsupported_media_type(true))
    } else {
        Ok(())
    }
}

fn combined(headers: &HeaderMap, name: HeaderName) -> Result<Option<Vec<u8>>, Problem> {
    let mut combined = Vec::new();
    let mut present = false;
    for field in headers.get_all(name).iter() {
        let extra = field.as_bytes().len() + usize::from(present);
        if extra > MAX_BYTES - combined.len() {
            return Err(Problem::bad_request());
        }
        if present {
            combined.push(b',');
        }
        combined.extend_from_slice(field.as_bytes());
        present = true;
    }
    Ok(present.then_some(combined))
}

fn trim_ows(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(|byte| matches!(byte, b' ' | b'\t')) {
        value = &value[1..];
    }
    while value.last().is_some_and(|byte| matches!(byte, b' ' | b'\t')) {
        value = &value[..value.len() - 1];
    }
    value
}

// RFC 9110 §5.6.1.2 requires ignoring reasonable empty list members. They
// have their own bound and do not count as media ranges or content codings.
fn list(input: &[u8]) -> Result<Vec<&[u8]>, ()> {
    if input.len() > MAX_BYTES {
        return Err(());
    }
    let mut parts = Vec::new();
    let mut empty = 0;
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in input.iter().copied().enumerate() {
        if escaped {
            escaped = false;
        } else if quoted && byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            quoted = !quoted;
        } else if !quoted && byte == b',' {
            push_part(&input[start..index], &mut parts, &mut empty)?;
            start = index + 1;
        }
    }
    if quoted || escaped {
        return Err(());
    }
    push_part(&input[start..], &mut parts, &mut empty)?;
    Ok(parts)
}

fn push_part<'a>(
    input: &'a [u8],
    parts: &mut Vec<&'a [u8]>,
    empty: &mut usize,
) -> Result<(), ()> {
    let input = trim_ows(input);
    if input.is_empty() {
        *empty += 1;
        if *empty > MAX_EMPTY_ELEMENTS {
            return Err(());
        }
    } else {
        if parts.len() == MAX_RANGES {
            return Err(());
        }
        parts.push(input);
    }
    Ok(())
}

fn token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.'
                | b'^' | b'_' | b'`' | b'|' | b'~'
        )
}

struct Cursor<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn ows(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.index += 1;
        }
    }

    fn take(&mut self, expected: u8) -> Result<(), ()> {
        if self.peek() != Some(expected) {
            return Err(());
        }
        self.index += 1;
        Ok(())
    }

    fn token(&mut self) -> Result<&'a [u8], ()> {
        let start = self.index;
        while self.peek().is_some_and(token_byte) {
            self.index += 1;
        }
        if self.index == start {
            return Err(());
        }
        Ok(&self.bytes[start..self.index])
    }

    fn value(&mut self) -> Result<(Vec<u8>, bool), ()> {
        if self.peek() != Some(b'"') {
            return Ok((self.token()?.to_vec(), false));
        }
        self.index += 1;
        let mut value = Vec::new();
        while let Some(byte) = self.peek() {
            self.index += 1;
            match byte {
                b'"' => return Ok((value, true)),
                b'\\' => {
                    let escaped = self.peek().ok_or(())?;
                    if !matches!(escaped, b'\t' | b' '..=b'~' | 0x80..=0xff) {
                        return Err(());
                    }
                    self.index += 1;
                    value.push(escaped);
                }
                b'\t' | b' ' | b'!' | b'#'..=b'[' | b']'..=b'~' | 0x80..=0xff => {
                    value.push(byte);
                }
                _ => return Err(()),
            }
        }
        Err(())
    }
}

fn parse_media(input: &[u8], range: bool) -> Result<Media, ()> {
    if input.len() > MAX_BYTES {
        return Err(());
    }
    let mut cursor = Cursor {
        bytes: trim_ows(input),
        index: 0,
    };
    let kind = cursor.token()?.to_ascii_lowercase();
    cursor.take(b'/')?;
    let subtype = cursor.token()?.to_ascii_lowercase();
    if (kind == b"*" && subtype != b"*")
        || (!range && (kind == b"*" || subtype == b"*"))
    {
        return Err(());
    }
    let mut parameters: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut quality = None;
    let mut empty = 0;
    loop {
        cursor.ows();
        if cursor.peek().is_none() {
            break;
        }
        cursor.take(b';')?;
        cursor.ows();
        // RFC 9110's parameters grammar permits absent parameters after ';'.
        if matches!(cursor.peek(), None | Some(b';')) {
            empty += 1;
            if empty > MAX_EMPTY_ELEMENTS {
                return Err(());
            }
            continue;
        }
        if parameters.len() + usize::from(quality.is_some()) == MAX_PARAMETERS {
            return Err(());
        }
        let name = cursor.token()?.to_ascii_lowercase();
        cursor.take(b'=')?;
        let (value, quoted) = cursor.value()?;
        if range && name == b"q" {
            if quality.is_some() || quoted {
                return Err(());
            }
            quality = Some(parse_quality(&value)?);
        } else {
            if parameters.iter().any(|(existing, _)| existing == &name) {
                return Err(());
            }
            parameters.push((name, value));
        }
    }
    Ok(Media {
        kind,
        subtype,
        parameters,
        quality: quality.unwrap_or(1000),
    })
}

fn parse_quality(value: &[u8]) -> Result<u16, ()> {
    let Some((&whole, rest)) = value.split_first() else {
        return Err(());
    };
    if !matches!(whole, b'0' | b'1') {
        return Err(());
    }
    let fraction = if rest.is_empty() {
        rest
    } else {
        rest.strip_prefix(b".").ok_or(())?
    };
    if fraction.len() > 3
        || !fraction.iter().all(u8::is_ascii_digit)
        || (whole == b'1' && fraction.iter().any(|byte| *byte != b'0'))
    {
        return Err(());
    }
    let mut quality = u16::from(whole - b'0') * 1000;
    let mut multiplier = 100;
    for byte in fraction {
        quality += u16::from(*byte - b'0') * multiplier;
        multiplier /= 10;
    }
    Ok(quality)
}

fn matches(range: &Media, representation: &Media) -> Option<(u8, usize)> {
    if range.kind != b"*" && range.kind != representation.kind {
        return None;
    }
    if range.subtype != b"*" && range.subtype != representation.subtype {
        return None;
    }
    for (name, value) in &range.parameters {
        let (_, offered) = representation
            .parameters
            .iter()
            .find(|(offered_name, _)| offered_name == name)?;
        let equal = if name == b"charset" {
            value.eq_ignore_ascii_case(offered)
        } else {
            value == offered
        };
        if !equal {
            return None;
        }
    }
    let kind = if range.kind == b"*" {
        0
    } else if range.subtype == b"*" {
        1
    } else {
        2
    };
    Some((kind, range.parameters.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderValue, StatusCode};
    use axum::response::IntoResponse;

    fn accept(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT, HeaderValue::from_str(value).unwrap());
        headers
    }

    fn status<T: std::fmt::Debug>(result: Result<T, Problem>) -> StatusCode {
        result.unwrap_err().into_response().status()
    }

    #[test]
    fn media_quality_uses_exact_grammar_and_specific_exclusions() {
        for (source, expected) in [
            ("0", 0), ("0.", 0), ("0.001", 1), ("0.1", 100),
            ("0.12", 120), ("0.999", 999), ("1", 1000), ("1.000", 1000),
        ] {
            assert_eq!(parse_quality(source.as_bytes()), Ok(expected));
        }
        for source in ["", ".1", "00", "01", "1.001", "0.0000", "-1", "NaN", "1e0"] {
            assert!(parse_quality(source.as_bytes()).is_err(), "{source}");
        }
        let offered = ["application/json", "application/geo+json"];
        assert_eq!(negotiate(&HeaderMap::new(), &offered).unwrap(), 0);
        assert_eq!(negotiate(&accept("application/json;q=0, */*;q=1"), &offered).unwrap(), 1);
        assert_eq!(negotiate(&accept("application/json;q=0.1, application/*;q=0.9"), &offered).unwrap(), 1);
        assert_eq!(negotiate(&accept("application/json;q=0, application/json;q=1"), &offered).unwrap(), 0);
        assert_eq!(status(negotiate(&accept(""), &offered)), StatusCode::NOT_ACCEPTABLE);
        assert_eq!(status(negotiate(&accept("*/*;q=0"), &offered)), StatusCode::NOT_ACCEPTABLE);
        assert_eq!(status(negotiate(&HeaderMap::new(), &[])), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(status(negotiate(&HeaderMap::new(), &["application/"])), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn media_parameters_preserve_quotes_values_and_weight_order() {
        let offered = [
            "application/json;profile=\"a;b,c\\\"d\";charset=utf-8",
            "application/json;profile=plain;charset=utf-8",
        ];
        for (source, expected) in [
            ("APPLICATION/JSON;PROFILE=\"a;b,c\\\"d\";CHARSET=UTF-8;q=1", 0),
            ("application/json;q=1;profile=plain;charset=UtF-8", 1),
            ("application/json;profile=\"plain\"", 1),
            ("application/json;profile=\"a;b,c\\\"d\";q=0, application/json", 1),
            ("application/json;q=0, application/json;profile=plain", 1),
            (",,application/json;;;profile=plain;,", 1),
        ] {
            assert_eq!(negotiate(&accept(source), &offered).unwrap(), expected, "{source}");
        }
        assert_eq!(status(negotiate(&accept("application/json;profile=Plain"), &offered)), StatusCode::NOT_ACCEPTABLE);
        for source in [
            "application/json;profile=\"unterminated", "application/json;q=\"1\"",
            "application/json;q=1;Q=0", "application/json;profile=plain;PROFILE=plain",
            "application/json;q =1", "application/json;q= 1", "*/json", "application /json",
        ] {
            assert_eq!(status(negotiate(&accept(source), &offered)), StatusCode::BAD_REQUEST, "{source}");
        }
    }

    #[test]
    fn media_lists_and_parameters_enforce_parser_bounds() {
        let offered = ["application/json", "application/geo+json"];
        let mut headers = accept("application/json;q=0");
        headers.append(header::ACCEPT, HeaderValue::from_static("application/geo+json"));
        assert_eq!(negotiate(&headers, &offered).unwrap(), 1);
        let maximum = vec!["application/json"; MAX_RANGES].join(",");
        assert_eq!(negotiate(&accept(&maximum), &offered).unwrap(), 0);
        assert_eq!(status(negotiate(&accept(&format!("{maximum},application/json")), &offered)), StatusCode::BAD_REQUEST);
        let parameters = (0..MAX_PARAMETERS).map(|index| format!(";p{index}=x")).collect::<String>();
        assert!(parse_media(format!("application/json{parameters}").as_bytes(), true).is_ok());
        assert!(parse_media(format!("application/json{parameters};overflow=x").as_bytes(), true).is_err());
        let oversized = format!("application/json;profile=\"{}\"", "x".repeat(MAX_BYTES));
        assert_eq!(status(negotiate(&accept(&oversized), &offered)), StatusCode::BAD_REQUEST);
        assert_eq!(status(negotiate(&accept(&",".repeat(MAX_EMPTY_ELEMENTS)), &offered)), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn media_json_and_coding_do_not_silently_relabel_input() {
        let mut headers = HeaderMap::new();
        assert_eq!(status(check_json_media(&headers)), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        for value in ["application/json", "APPLICATION/JSON;charset=utf-8", "application/json;profile=\"ignored\"", "application/json;;"] {
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_str(value).unwrap());
            assert!(check_json_media(&headers).is_ok());
        }
        headers.append(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
        assert_eq!(status(check_json_media(&headers)), StatusCode::BAD_REQUEST);
        headers.remove(header::CONTENT_TYPE);
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/sml+json"));
        assert_eq!(status(check_json_media(&headers)), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert!(check_coding(&headers).is_ok());
        for value in ["", "identity", "IDENTITY, ,identity,"] {
            headers.insert(header::CONTENT_ENCODING, HeaderValue::from_str(value).unwrap());
            assert!(check_coding(&headers).is_ok());
        }
        headers.append(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        let unsupported = check_coding(&headers).unwrap_err().into_response();
        assert_eq!(unsupported.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(unsupported.headers().get(header::ACCEPT_ENCODING).unwrap(), "identity");
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("identity;foo=bar"));
        assert_eq!(status(check_coding(&headers)), StatusCode::BAD_REQUEST);
    }
}
