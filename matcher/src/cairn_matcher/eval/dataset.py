"""The labelled-dataset format the harness measures, plus its loader.

Ground truth is expressed as ENTITY CLUSTERS: records grouped by the real person they
describe. Within-cluster record pairs are true matches; cross-cluster pairs are true
non-matches. That avoids hand-labelling O(n^2) pairs.

A dataset record deliberately mirrors the projection-row SHAPE the matcher already
operates over, so the pure scorer eval and the DB blocking eval both derive from one
shape with no parallel construction logic (see record_to_candidate / blocking_eval).
"""

from collections.abc import Mapping, Sequence
from dataclasses import dataclass


class DatasetError(ValueError):
    """The dataset JSON is structurally invalid (missing/duplicate ids, wrong shape).

    Raised loudly rather than silently tolerated (house rule #5): a malformed eval set
    would otherwise produce quietly-wrong metrics.
    """


@dataclass(frozen=True)
class DatasetRecord:
    """One patient record as projection-shaped field dicts. Every field is optional
    except record_id; absence is a safe, gradeable absence (principle 4), not an error.

    dob: {"value": ISO str, "precision": "year"|"month"|"day", "provenance_rank": int}
    sex_at_birth: {"value": str, "provenance_rank": int}
    names: tuple of {"value": display str, "provenance_rank": int}
    identifiers: tuple of {"system": str, "match_key": str, "value": str}
    """

    record_id: str
    dob: Mapping | None = None
    sex_at_birth: Mapping | None = None
    names: tuple[Mapping, ...] = ()
    identifiers: tuple[Mapping, ...] = ()


@dataclass(frozen=True)
class EntityCluster:
    """All records that describe ONE real person — the ground-truth grouping."""

    entity_id: str
    records: tuple[DatasetRecord, ...]


@dataclass(frozen=True)
class LabelledDataset:
    """A named set of entity clusters: the unit the harness evaluates."""

    name: str
    entities: tuple[EntityCluster, ...]
    description: str = ""

    def all_records(self) -> tuple[DatasetRecord, ...]:
        """Every record across all clusters, in cluster-then-record declaration order."""
        return tuple(r for e in self.entities for r in e.records)


def _record_from(obj: Mapping) -> DatasetRecord:
    """Shape one record dict into a DatasetRecord; require a non-empty record_id."""
    record_id = obj.get("record_id")
    if not isinstance(record_id, str) or not record_id:
        raise DatasetError(f"each record needs a non-empty string record_id, got {obj!r}")
    return DatasetRecord(
        record_id=record_id,
        dob=obj.get("dob"),
        sex_at_birth=obj.get("sex_at_birth"),
        names=tuple(obj.get("names", ())),
        identifiers=tuple(obj.get("identifiers", ())),
    )


def load_dataset(obj: Mapping) -> LabelledDataset:
    """Parse an in-memory dataset mapping (already JSON-decoded) into typed clusters.

    Validates the two invariants the harness depends on: there is an `entities` list,
    and every record_id is unique across the whole dataset (pairs are keyed by id).
    """
    entities_raw = obj.get("entities")
    if not isinstance(entities_raw, Sequence) or isinstance(entities_raw, (str, bytes)):
        raise DatasetError("dataset needs an 'entities' list")

    seen_ids: set[str] = set()
    entities: list[EntityCluster] = []
    for ent in entities_raw:
        entity_id = ent.get("entity_id")
        if not isinstance(entity_id, str) or not entity_id:
            raise DatasetError(f"each entity needs a non-empty entity_id, got {ent!r}")
        records = tuple(_record_from(r) for r in ent.get("records", ()))
        for r in records:
            if r.record_id in seen_ids:
                raise DatasetError(f"duplicate record_id across dataset: {r.record_id!r}")
            seen_ids.add(r.record_id)
        entities.append(EntityCluster(entity_id=entity_id, records=records))

    return LabelledDataset(
        name=str(obj.get("name", "")),
        description=str(obj.get("description", "")),
        entities=tuple(entities),
    )
