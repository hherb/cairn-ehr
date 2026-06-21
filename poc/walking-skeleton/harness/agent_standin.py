"""Spike 0002 advisory-agent stand-in (fit-for-purpose Python, §9.1).

Mimics the kastellan integration *contract*: it loads its actor identity + skill
epoch, reads a provenance blob reference, computes a trivial urgency score, signs
the event with its own Ed25519 key (via `cairn-sync sign-stdin`, so Rust owns the
canonical COSE encoding), and authors the advisory ONLY through submit_event. It
never touches event_log directly.
"""
import json
import subprocess
import sys
import uuid

import psycopg


def _body(event_type, patient_id, schema, payload, attachments, kid):
    return {
        "event_id": str(uuid.uuid4()),  # UUIDv7 minted in Rust normally; v4 ok for the spike body
        "patient_id": patient_id,
        "event_type": event_type,
        "schema_version": schema,
        "hlc": {"wall": 1, "counter": 0, "node_origin": "agent"},
        "t_effective": None,
        "signer_key_id": kid,
        "contributors": [{"actor_id": "agent", "role": "triaged"}],
        "payload": payload,
        "attachments": attachments,
    }


def _sign(bin_path, key_path, body):
    p = subprocess.run([bin_path, "sign-stdin", "--key", key_path],
                       input=json.dumps(body).encode(), capture_output=True)
    if p.returncode != 0:
        raise RuntimeError(f"sign-stdin failed: {p.stderr.decode()}")
    return p.stdout.decode().strip()


def author(conn_str, bin_path, key_path, blob_addr_hex, patient_id):
    """Author one advisory through submit_event. Returns the new event_id."""
    # The agent must sign with the kid it is enrolled under; read it from the key
    # by signing a probe and extracting the COSE key_id via the DB's cairn_body.
    with psycopg.connect(conn_str, autocommit=True) as db:
        probe = _body("advisory.added", patient_id, "advisory/1", {}, [], kid="")
        # First pass: sign to learn our kid (cairn_body exposes signer_key_id).
        signed_hex = _sign(bin_path, key_path, probe)
        row = db.execute("SELECT cairn_body(decode(%s,'hex')) ->> 'signer_key_id'",
                         (signed_hex,)).fetchone()
        kid = row[0]

        # urgency score = a trivial deterministic function of the blob address.
        urgency = (int(blob_addr_hex[:2], 16) % 5) + 1
        body = _body(
            "advisory.added", patient_id, "advisory/1",
            {"urgency": urgency, "summary": "triage advisory (stand-in)"},
            [{"alg": "blake3", "digest_hex": blob_addr_hex,
              "media_type": "message/rfc822", "descriptor": "source mail", "byte_len": 1}],
            kid=kid,
        )
        signed_hex = _sign(bin_path, key_path, body)
        row = db.execute("SELECT submit_event(decode(%s,'hex'))", (signed_hex,)).fetchone()
        return row[0]


if __name__ == "__main__":
    # CLI: author --conn … --bin … --key … --blob-addr … --patient …
    args = dict(zip(sys.argv[2::2], sys.argv[3::2])) if len(sys.argv) > 2 else {}
    if sys.argv[1] == "author":
        eid = author(args["--conn"], args["--bin"], args["--key"],
                     args["--blob-addr"], args["--patient"])
        print(eid)
