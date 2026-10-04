#!/usr/bin/env python3
"""Verify the pinned UCUM 2.1 source, derived lookup and required notices offline.

Uses only the hosted runner's Python standard library. --emit-table reproduces
the table on stdout; it never writes an original or downloads a replacement.
--self-test proves each asset guard rejects an independently changed byte.
"""

import argparse
import hashlib
import html
import re
from pathlib import Path
import xml.etree.ElementTree as ET


ORIGINALS = {
    "ucum-essence.xml": "774b99e5ec13c6d9f3bc875bb91f86f5a4da08020c645adf812039883f4beb8c",
    "ucum-source.xml": "78974be02ec3dc44985f4833a30323d22bf18debae2ff11c222b71b51e27f6fa",
}
NAMESPACE = "{http://unitsofmeasure.org/ucum-essence}"


def table_from(essence):
    root = ET.fromstring(essence)
    if root.tag != NAMESPACE + "root" or root.attrib["version"] != "2.1":
        raise ValueError("unexpected UCUM basis")
    rows = []
    codes = {"prefix": set(), "atom": set()}
    for node in root:
        kind = node.tag.removeprefix(NAMESPACE)
        if kind not in {"prefix", "base-unit", "unit"}:
            raise ValueError("unexpected dictionary entry")
        code = node.attrib["Code"]
        group = "prefix" if kind == "prefix" else "atom"
        if code in codes[group]:
            raise ValueError("duplicate dictionary code")
        codes[group].add(code)
        value = node.find(NAMESPACE + "value")
        metric = kind == "base-unit" or node.get("isMetric") == "yes"
        special = node.get("isSpecial") == "yes" or (
            value is not None and value.find(NAMESPACE + "function") is not None
        )
        # Keep code, definition value and unit in full, including absent fields.
        # The last field prevents an absent unit from becoming trailing whitespace.
        fields = [
            kind,
            code,
            "yes" if metric else "no",
            "yes" if special else "no",
            value.get("value", "") if value is not None else "",
            value.get("Unit", "") if value is not None else "",
            ".",
        ]
        if any("\t" in field or "\n" in field for field in fields):
            raise ValueError("dictionary field requires a different representation")
        rows.append("\t".join(fields))
    if len(codes["prefix"]) != 24 or len(codes["atom"]) != 310:
        raise ValueError("unexpected dictionary inventory")
    return ("\n".join(rows) + "\n").encode("utf-8")


def plain_text(markup):
    text = html.unescape(re.sub(r"<[^>]*>", "", markup))
    text = "\n".join(line.strip() for line in text.splitlines())
    return re.sub(r"\n{3,}", "\n\n", text).strip() + "\n"


def notices_from(source):
    # Source XML has its historical DTD; do not load it or any external entity.
    # Extract the known, digest-checked licence block as text, without XML IO.
    match = re.search(
        r'<div1 ignore="no" id="license">(.*?)</div1>',
        source.decode("utf-8"),
        re.DOTALL,
    )
    if match is None:
        raise ValueError("missing original licence")
    markup = match.group(1)
    clauses = iter(range(1, 14))
    numbered = re.sub(r"<item>", lambda _: str(next(clauses)) + ". ", markup)
    licence = "UCUM version 2.1\n\n" + plain_text(numbered)
    quotes = re.findall(r"<quote>(.*?)</quote>", markup, re.DOTALL)
    if len(quotes) != 2:
        raise ValueError("unexpected short notice")
    notice = "\n\n".join(plain_text(quote).strip() for quote in quotes) + "\n"
    return licence.encode("utf-8"), notice.encode("utf-8")


def verify(assets):
    for name, digest in ORIGINALS.items():
        if hashlib.sha256(assets[name]).hexdigest() != digest:
            raise ValueError("original digest: " + name)
    if table_from(assets["ucum-essence.xml"]) != assets["table.tsv"]:
        raise ValueError("derived table differs from the complete original")
    licence, notice = notices_from(assets["ucum-source.xml"])
    if licence != assets["license.txt"]:
        raise ValueError("licence differs from the original text")
    if notice != assets["UCUM_short_license.txt"]:
        raise ValueError("short notice differs from the original text")


def self_test(assets):
    for name in assets:
        changed = dict(assets)
        changed[name] = assets[name] + b"!"
        try:
            verify(changed)
        except ValueError:
            print("UCUM asset mutation rejected: " + name)
        else:
            raise AssertionError("UCUM asset mutation passed: " + name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--emit-table", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    if {entry.name for entry in (root / "originals").iterdir()} != set(ORIGINALS):
        raise ValueError("unexpected original-file inventory")
    assets = {name: (root / "originals" / name).read_bytes() for name in ORIGINALS}
    assets.update(
        {name: (root / name).read_bytes()
         for name in ("table.tsv", "license.txt", "UCUM_short_license.txt")}
    )
    verify(assets)
    if args.emit_table:
        print(table_from(assets["ucum-essence.xml"]).decode("utf-8"), end="")
        return
    print("UCUM 2.1: 24 prefixes, 310 atoms; original digests, table and notices verified")
    if args.self_test:
        self_test(assets)


if __name__ == "__main__":
    main()
