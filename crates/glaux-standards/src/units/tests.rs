use super::*;

#[test]
fn ucum_source_examples() {
    // UCUM 2.1 §§4–10, 12 and the tables supply these independent expectations.
    // In particular a dot joins integer factors; it is not a decimal separator.
    for code in [
        "1",
        "0001",
        "2.5",
        "2+10",
        "2-10",
        "10*3",
        "10^-6",
        "kg.m/s2",
        "m/s/s",
        "/s",
        "m/(s.s)",
        "m0",
        "s+2",
        "cm3",
        "mm[Hg]",
        "m[H2O]",
        "[in_i'H2O]",
        "[ft_i]/s",
        "daL",
        "uL",
        "KiBy",
        "TiBy",
        "[IU]/mL",
        "m[iU]",
        "mg%",
        "%{vol}",
        "kg{total}",
        "{RBC}",
        "1{ratio}",
        "{a[b](c)/d+e-f.g=1}",
        "{}",
        "{RBC}/uL",
        "[HPF]",
        "[arb'U]/mL",
    ] {
        assert_eq!(validate_code(code), Ok(()), "{code}");
    }
}

#[test]
fn ucum_rejects_invalid_codes() {
    // Case-sensitive codes, official metric predicates, punctuation and grammar
    // are distinct failure classes; a generic printable-string check fails these.
    for code in [
        "",
        " ",
        "m s",
        "m\ts",
        "m\n",
        "µm",
        "μm",
        "°C",
        "\0m",
        "m\u{7f}",
        "meter",
        "Celcius",
        "KG",
        "CEL",
        "ohm",
        "kmin",
        "k[ft_i]",
        "k%",
        "kkg",
        "mmmol",
        "12m",
        "0",
        "000",
        "m*kg",
        ".m",
        "m.",
        "m//s",
        "//m",
        "m..s",
        "m/",
        "m+",
        "m--2",
        "m2-3",
        "m^2",
        "-1",
        "()",
        "(m",
        "m)",
        "m(s)",
        "(m/s)2",
        "(/s)",
        "m{a}{b}",
        "m{a}2",
        "m{bad annotation}",
        "m{bad{nested}}",
        "m{open",
        "m}",
        "[[ft_i]]",
        "[ft_i",
        "[unknown]",
        "m=s",
        "\"m\"",
        "Cel/unknown",
    ] {
        assert_eq!(validate_code(code), Err(UnitError::Invalid), "{code}");
    }
}

#[test]
fn ucum_dictionary_2_1() {
    // This tests broad lookup/parser coverage; the independent Python digest and
    // re-derivation check, not this loop, proves which dictionary is included.
    let dictionary = dictionary();
    assert_eq!(dictionary.prefixes.len(), 24);
    assert_eq!(dictionary.atoms.len(), 310);
    assert_eq!(
        dictionary
            .atoms
            .values()
            .filter(|atom| atom.special)
            .count(),
        21
    );
    for (code, atom) in &dictionary.atoms {
        assert_eq!(validate_code(code), Ok(()), "atom {code}");
        if atom.metric {
            for prefix in &dictionary.prefixes {
                let prefixed = format!("{prefix}{code}");
                assert_eq!(validate_code(&prefixed), Ok(()), "prefix {prefixed}");
            }
        }
    }
}

#[test]
fn ucum_special_scales_explicit() {
    for code in [
        "Cel",
        "mCel",
        "Cel1",
        "Cel+01",
        "(Cel)",
        "Cel{sensor}",
        "[degF]",
        "dB",
        "dB[V]",
        "[pH]",
        "bit_s",
        "[hp'_X]",
        "[m/s2/Hz^(1/2)]",
    ] {
        assert_eq!(validate_code(code), Ok(()), "{code}");
    }
    // Unit syntax alone is not authority for algebra on non-ratio scales.
    // Even known-valid scalar scaling is explicitly outside this bounded check.
    for code in ["Cel/s", "Cel2", "Cel0", "Cel-1", "2.Cel", "Cel/1", "/Cel"] {
        assert_eq!(validate_code(code), Err(UnitError::Unsupported), "{code}");
    }
}

#[test]
fn ucum_resource_limits() {
    assert_eq!(
        validate_code(&"m".repeat(MAX_CODE_BYTES + 1)),
        Err(UnitError::Limit)
    );
    let maximum_annotation = format!("{{{}}}", "x".repeat(MAX_CODE_BYTES - 2));
    assert_eq!(validate_code(&maximum_annotation), Ok(()));
    let nested = format!("{}m{}", "(".repeat(MAX_NESTING), ")".repeat(MAX_NESTING));
    assert_eq!(validate_code(&nested), Ok(()));
    assert_eq!(validate_code(&format!("({nested})")), Err(UnitError::Limit));
    let components = vec!["m"; MAX_COMPONENTS].join(".");
    assert_eq!(validate_code(&components), Ok(()));
    assert_eq!(
        validate_code(&format!("{components}.m")),
        Err(UnitError::Limit)
    );
    assert_eq!(validate_code("m999999"), Ok(()));
    assert_eq!(validate_code("m1000000"), Err(UnitError::Limit));
    assert_eq!(validate_code("m-1000000"), Err(UnitError::Limit));
    assert_eq!(validate_code(&"9".repeat(MAX_CODE_BYTES)), Ok(()));
}

#[test]
fn ucum_time_codes_use_temporal_atoms() {
    // Independently transcribed from the pinned essence's property=time entries
    // and prefix table. No conversion factors or calendar rules are inferred.
    for code in [
        "s", "min", "h", "d", "wk", "a_t", "a_j", "a_g", "a", "mo_s", "mo_j", "mo_g", "mo",
    ] {
        assert_eq!(validate_time_code(code), Ok(()), "temporal atom {code}");
    }
    for prefix in [
        "Y", "Z", "E", "P", "T", "G", "M", "k", "h", "da", "d", "c", "m", "u", "n", "p", "f", "a",
        "z", "y", "Ki", "Mi", "Gi", "Ti",
    ] {
        let code = format!("{prefix}s");
        assert_eq!(validate_time_code(&code), Ok(()), "prefixed second {code}");
    }
    for code in [
        "s1",
        "s+1",
        "h+000001",
        "(s)",
        "((ms{ticks}))",
        "s{1/m}",
        "(s{)})",
        "mo{calendar-label-is-inert}",
    ] {
        assert_eq!(
            validate_time_code(code),
            Ok(()),
            "temporal expression {code}"
        );
    }
}

#[test]
fn ucum_time_codes_reject_or_defer_other_units() {
    // Passing the generic code check does not establish a temporal unit.
    for code in [
        "m",
        "kg",
        "Hz",
        "mHz",
        "1",
        "Cel",
        "dB",
        "S",
        "{seconds}",
        "1{s}",
        "2+10",
        "s0",
        "s2",
        "s-1",
        "ms0",
        "a2",
    ] {
        assert_eq!(validate_code(code), Ok(()), "valid UCUM {code}");
        assert_eq!(
            validate_time_code(code),
            Err(UnitError::Invalid),
            "not Time {code}"
        );
    }
    for code in [
        "1.s", "s/1", "s.m/m", "Hz-1", "1/Hz", "m2/s", "[S]", "(s).(s)", "{x}.s", "s/{x}",
    ] {
        assert_eq!(validate_code(code), Ok(()), "valid UCUM {code}");
        assert_eq!(
            validate_time_code(code),
            Err(UnitError::Unsupported),
            "unresolved {code}"
        );
    }
    for code in ["sec", "yr", "kmin", "s ", "s/typo", "s{open", "(s)2"] {
        assert_eq!(
            validate_time_code(code),
            Err(UnitError::Invalid),
            "malformed {code}"
        );
    }
    assert_eq!(validate_time_code("Cel/s"), Err(UnitError::Unsupported));
    assert_eq!(validate_time_code("s1000000"), Err(UnitError::Limit));
    assert_eq!(
        validate_time_code(&format!("s{{{}}}", "x".repeat(MAX_CODE_BYTES))),
        Err(UnitError::Limit)
    );
}
