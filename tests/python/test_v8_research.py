from __future__ import annotations

import csv
import hashlib
import json
from pathlib import Path
from typing import Any


V8_DATA = Path(__file__).parents[1] / "data" / "dgn" / "v8"
MANIFEST = V8_DATA / "manifest.json"


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _is_3d_wkt(wkt: str) -> bool:
    geometry_type = wkt.partition(" ")[0]
    return wkt.startswith(f"{geometry_type} Z ")


def test_v8_manifest_matches_fixed_artifacts_and_oracle() -> None:
    manifest: dict[str, Any] = json.loads(MANIFEST.read_text(encoding="utf-8"))
    assert manifest["schema_version"] == 1
    artifacts = {artifact["path"]: artifact for artifact in manifest["artifacts"]}

    for name, artifact in artifacts.items():
        path = V8_DATA / name
        assert path.stat().st_size == artifact["bytes"]
        assert _sha256(path) == artifact["sha256"]

    container = artifacts["test_dgnv8.dgn"]["container_expectations"]
    streams = container["streams"]
    assert len(streams) == container["stream_count"]
    assert sum(stream["bytes"] for stream in streams) == container[
        "total_stream_bytes"
    ]
    assert [stream["path"] for stream in streams] == sorted(
        stream["path"] for stream in streams
    )

    oracle = artifacts["test_dgnv8_ref.csv"]["semantic_expectations"]
    with (V8_DATA / "test_dgnv8_ref.csv").open(
        newline="", encoding="utf-8"
    ) as stream:
        rows = list(csv.DictReader(stream))

    assert len(rows) == oracle["feature_count"]
    assert sum(not _is_3d_wkt(row["WKT"]) for row in rows) == oracle[
        "feature_dimensions"
    ]["2d"]
    assert sum(_is_3d_wkt(row["WKT"]) for row in rows) == oracle[
        "feature_dimensions"
    ]["3d"]
    assert sorted({int(row["Type"]) for row in rows}) == oracle["type_codes"]
    assert sorted(
        {
            row["Text"]
            for row in rows
            if row["Text"]
            and any(not character.isascii() for character in row["Text"])
        }
    ) == oracle["unicode_text"]
