"""Pure tests for the eval dataset value types and loader."""

import pytest

from cairn_matcher.eval.dataset import (
    DatasetError,
    DatasetRecord,
    EntityCluster,
    LabelledDataset,
    load_dataset,
)

_MINIMAL = {
    "name": "tiny",
    "entities": [
        {"entity_id": "e1", "records": [
            {"record_id": "r1", "dob": {"value": "1990-05-12", "precision": "day", "provenance_rank": 70}},
            {"record_id": "r2", "names": [{"value": "Alex Nguyen", "provenance_rank": 30}]},
        ]},
        {"entity_id": "e2", "records": [{"record_id": "r3"}]},
    ],
}


def test_load_dataset_builds_typed_tree():
    ds = load_dataset(_MINIMAL)
    assert isinstance(ds, LabelledDataset)
    assert ds.name == "tiny"
    assert len(ds.entities) == 2
    assert isinstance(ds.entities[0], EntityCluster)
    assert isinstance(ds.entities[0].records[0], DatasetRecord)
    assert ds.entities[0].records[0].record_id == "r1"
    assert ds.entities[0].records[0].dob == {"value": "1990-05-12", "precision": "day", "provenance_rank": 70}


def test_all_records_flattens_in_order():
    ds = load_dataset(_MINIMAL)
    assert [r.record_id for r in ds.all_records()] == ["r1", "r2", "r3"]


def test_missing_record_id_raises():
    bad = {"name": "x", "entities": [{"entity_id": "e", "records": [{"dob": {}}]}]}
    with pytest.raises(DatasetError):
        load_dataset(bad)


def test_duplicate_record_id_raises():
    bad = {"name": "x", "entities": [
        {"entity_id": "e1", "records": [{"record_id": "dup"}]},
        {"entity_id": "e2", "records": [{"record_id": "dup"}]},
    ]}
    with pytest.raises(DatasetError):
        load_dataset(bad)


def test_missing_entities_key_raises():
    with pytest.raises(DatasetError):
        load_dataset({"name": "x"})


def test_name_without_value_raises_located_dataset_error():
    # A name dict missing "value" must fail loudly at load time (record_to_candidate /
    # the seeder index it directly) rather than as an opaque KeyError downstream.
    bad = {"name": "x", "entities": [{"entity_id": "e", "records": [
        {"record_id": "r1", "names": [{"provenance_rank": 30}]}]}]}
    with pytest.raises(DatasetError, match="r1"):
        load_dataset(bad)


def test_identifier_without_required_keys_raises():
    bad = {"name": "x", "entities": [{"entity_id": "e", "records": [
        {"record_id": "r1", "identifiers": [{"system": "mrn"}]}]}]}
    with pytest.raises(DatasetError, match="identifier"):
        load_dataset(bad)
