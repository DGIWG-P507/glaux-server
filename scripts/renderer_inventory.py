"""Verify the locally served, byte-pinned Swagger UI subset without a network."""

import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "crates/glaux-server/assets/swagger-ui"
COMMIT = "cfd4a6c3cbaeeb7c13a8bada7c754de42d78cd5b"
EXPECTED = {
    "swagger-ui-bundle.js": (1585988, "62df541529080464a7660adc793eab7128c6193ce3be24ddc1e0e0a4a63edc2f"),
    "swagger-ui.css": (186154, "1ac324f7dcd27e4b9386b4bd6421271ec147e922a22c05ba24b11515e9aa6321"),
    "swagger-ui-bundle.js.LICENSE.txt": (4442, "63818894e4b04cd0e3180d9cb20761e227a939121e7484f8e1d528227c756f89"),
    "LICENSE": (11358, "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30"),
    "NOTICE": (55, "0d20d1adef18aee3f40dd258172155521ce702ac445cb5f7b7d60ed32dad2fb2"),
}


def renderer_inventory():
    pin = json.loads((ASSETS / "manifest.json").read_text())
    if (pin["name"], pin["version"], pin["repository"], pin["commit"], pin["project_license"]) != (
        "Swagger UI", "5.33.0", "https://github.com/swagger-api/swagger-ui", COMMIT, "Apache-2.0",
    ):
        raise AssertionError("Renderer identity changed without inventory review")
    if {item.name for item in ASSETS.iterdir()} != set(EXPECTED) | {"manifest.json"}:
        raise AssertionError("Renderer asset inventory changed")
    entries = pin["files"]
    if len(entries) != len(EXPECTED) or {entry["path"] for entry in entries} != set(EXPECTED):
        raise AssertionError("Renderer manifest file set changed")
    for entry in entries:
        name = entry["path"]
        expected_source = "dist/" + name if name.startswith("swagger-ui") else name
        if entry["upstream"] != expected_source:
            raise AssertionError("Renderer upstream path changed")
        expected = EXPECTED[name]
        data = (ASSETS / name).read_bytes()
        if (entry["bytes"], entry["sha256"]) != expected or (len(data), hashlib.sha256(data).hexdigest()) != expected:
            raise AssertionError("Renderer bytes differ from reviewed upstream pin: " + name)
    return {**pin, "verification": "All five files match reviewed upstream bytes; no runtime fetch or npm install.",
            "licence_scope": "Project LICENSE/NOTICE and emitted bundled licence notices retained; not a complete transitive npm SBOM or legal approval."}


if __name__ == "__main__":
    print(json.dumps(renderer_inventory(), indent=2))
