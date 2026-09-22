"""Offline packaging controls, not schema validation."""
import copy
import hashlib
import json
from pathlib import Path
import shutil
import socket
import sys
import tempfile
import unittest

from check_corpus import CorpusError, check, deny_socket

SOURCE = Path(__file__).resolve().parents[1] / "crates/glaux-standards/corpus"
QUANTITY = "originals/csapi/swecommon/schemas/json/Quantity.json"


class CorpusTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="glaux-corpus-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name) / "corpus"
        shutil.copytree(SOURCE, self.root)

    def manifest(self):
        return json.loads((self.root / "manifest.json").read_text())

    def save(self, value):
        (self.root / "manifest.json").write_text(json.dumps(value), encoding="utf-8")

    def replace_json(self, relative, transform):
        # Update disposable digest too, so this reaches reference checks.
        path = self.root / relative
        value = json.loads(path.read_text())
        transform(value)
        data = json.dumps(value).encode()
        path.write_bytes(data)
        manifest = self.manifest()
        artifact = next(a for a in manifest["artifacts"] if a["path"] == relative)
        artifact.update(bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
        for mirror in artifact["verified_mirrors"]:
            mirror.update(bytes=len(data), sha256=artifact["sha256"])
        self.save(manifest)

    def test_baseline_inventory_and_recursive_fixture_pairs(self):
        result = check(self.root)
        for key, value in {"artifacts": 138, "schemas": 129, "fixtures": 23,
                           "expectations_true": 7, "expectations_false": 16}.items():
            self.assertEqual(result[key], value, key)
        self.assertGreater(result["references"], 200)
        # Authored fixture hygiene, not a replacement schema validator. The
        # source Count.json requires its own label independently of Quantity.
        def fixture(name):
            return json.loads((self.root / "fixtures" / name).read_text())

        def sample(record):
            return record["fields"][0]["elementType"]

        def count(record):
            return sample(record)["fields"][1]["fields"][0]

        expected_count = {"type": "Count", "name": "sequence", "label": "Sequence",
                          "definition": "urn:glaux:fixture:sequence", "value": 1}
        swe = fixture("swe-recursive.json")
        self.assertEqual(count(swe), expected_count)
        swe_negative = copy.deepcopy(swe)
        count(swe_negative)["value"] = "one"
        self.assertEqual(fixture("swe-recursive-invalid-count.json"), swe_negative)
        sml = fixture("sensorml-recursive.json")
        output = sml["components"][0]["components"][0]["outputs"][0]
        self.assertEqual(count(output), expected_count)
        sml_negative = copy.deepcopy(sml)
        negative_output = sml_negative["components"][0]["components"][0]["outputs"][0]
        del sample(negative_output)["fields"][0]["label"]
        self.assertEqual(fixture("sensorml-recursive-missing-quantity-label.json"), sml_negative)

    def test_known_official_bytes_not_only_manifest(self):
        # Independently retrieved official-source digests, recorded in #7.
        expected = {
            QUANTITY: "dc23d3496ae02a6d1de756d441aa12f248e74a1ff3feafda826640af04115b52",
            "originals/csapi/swecommon/schemas/json/encodings.json":
                "9d432bbec5ffebeda21d612ab1ec6b22d4e8011ac967820ba17c07b0391e73ab",
            "originals/geojson/Point.json":
                "35dd9cc5537e3a02a58ec63c22001508bd0c26036803f86004bb7cad0e9ad8b1",
        }
        for path, digest in expected.items():
            self.assertEqual(hashlib.sha256((self.root / path).read_bytes()).hexdigest(), digest)

    def test_altered_byte_rejected(self):
        path = self.root / QUANTITY
        data = bytearray(path.read_bytes())
        data[0] ^= 1
        path.write_bytes(data)
        with self.assertRaisesRegex(CorpusError, "SHA-256 mismatch.*Quantity.json"):
            check(self.root)

    def test_required_local_file_removed(self):
        (self.root / QUANTITY).unlink()
        with self.assertRaisesRegex(CorpusError, "Missing regular file.*Quantity.json"):
            check(self.root)

    def test_reference_target_removed_from_catalog_and_disk(self):
        (self.root / QUANTITY).unlink()
        manifest = self.manifest()
        manifest["artifacts"] = [a for a in manifest["artifacts"] if a["path"] != QUANTITY]
        self.save(manifest)
        with self.assertRaisesRegex(CorpusError, "Unpackaged schema URI.*Quantity.json"):
            check(self.root)

    def test_unknown_network_reference_rejected_without_fetch(self):
        self.replace_json(QUANTITY, lambda v: v.update({"$ref": "https://example.invalid/not-packaged.json"}))
        with self.assertRaisesRegex(CorpusError, "Unpackaged schema URI.*example.invalid"):
            check(self.root)

    def test_missing_fragment_despite_matching_digest(self):
        self.replace_json(QUANTITY, lambda v: v.update({"$ref": "#/$defs/does-not-exist"}))
        with self.assertRaisesRegex(CorpusError, "Missing JSON pointer target.*does-not-exist"):
            check(self.root)

    def test_missing_dynamic_anchor_rejected(self):
        self.replace_json("originals/json-schema/2020-12/meta/core.json",
                          lambda v: v.update({"$dynamicRef": "#no-such-anchor"}))
        with self.assertRaisesRegex(CorpusError, "Missing schema anchor.*no-such-anchor"):
            check(self.root)

    def test_duplicate_catalog_uri_rejected(self):
        manifest = self.manifest()
        schemas = [a for a in manifest["artifacts"] if a["kind"] == "schema"]
        schemas[1]["aliases"].append(schemas[0]["uri"])
        self.save(manifest)
        with self.assertRaisesRegex(CorpusError, "Duplicate artifact URI/alias"):
            check(self.root)

    def test_traversal_path_rejected(self):
        manifest = self.manifest()
        manifest["artifacts"][0]["path"] = "../outside.json"
        self.save(manifest)
        with self.assertRaisesRegex(CorpusError, "Unsafe relative path"):
            check(self.root)

    def test_symlink_rejected(self):
        path = self.root / QUANTITY
        target = self.root / "outside-target.json"
        target.write_bytes(path.read_bytes())
        path.unlink()
        path.symlink_to(target)
        with self.assertRaisesRegex(CorpusError, "Symlink/junction forbidden"):
            check(self.root)

    def test_extra_original_rejected(self):
        (self.root / "originals/unaccounted.json").write_text("{}")
        with self.assertRaisesRegex(CorpusError, "Originals inventory differs"):
            check(self.root)

    def test_empty_fixture_selection_rejected(self):
        (self.root / "fixtures/cases.json").write_text('{"cases": []}')
        with self.assertRaisesRegex(CorpusError, "Empty/missing fixture cases"):
            check(self.root)

    def test_duplicate_json_key_rejected(self):
        (self.root / "fixtures/binary-valid.json").write_text('{"type":"BinaryEncoding","type":"TextEncoding"}')
        with self.assertRaisesRegex(CorpusError, "Duplicate JSON key"):
            check(self.root)

    def test_socket_guard_itself_rejects_attempt(self):
        with self.assertRaisesRegex((CorpusError, RuntimeError), "[Oo]ffline|[Nn]etwork|[Ss]ocket"):
            socket.socket()


if __name__ == "__main__":
    sys.addaudithook(deny_socket)
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(CorpusTests)
    if suite.countTestCases() != 15:
        raise SystemExit("Required corpus control selection changed or is empty")
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    if result.testsRun != 15 or result.skipped or not result.wasSuccessful():
        raise SystemExit(1)
    print("15 packaging controls executed; schema validation expectations NOT executed.")
