//! Cairn walking skeleton — the thin sync daemon (Spike 0001 §3, §5).
//!
//! Set-union ship/apply over a tiny framed protocol (run over WireGuard; NoTls is
//! deliberate — the link is the transport). Two planes, exactly as the spec
//! separates them:
//!
//!   * **clinical plane** (`serve` events / `pull`): eager, small, high priority —
//!     ships signed event bytes; the receiver *verifies on apply* (Bet A2) and
//!     inserts idempotently (`ON CONFLICT DO NOTHING` — set-union, Bet A1).
//!   * **byte tier** (`serve` blob chunks / `blobd`): lazy, chunked, preemptible,
//!     separately budgeted — must never starve the clinical plane (Bet A4).
//!
//! This daemon carries NO merge logic (ADR-0001/§9.4): convergence is set-union +
//! the in-DB projection trigger. It only ships bytes, verifies, and applies.

use std::error::Error;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cairn_event::{blob_address, plaintext_twin, sign, verify_self_described, EventBody, Hlc, SigningKey};
use serde::{Deserialize, Serialize};

const SCHEMA: [(&str, &str); 3] = [
    ("001_envelope", include_str!("../../../db/001_envelope.sql")),
    ("002_projection", include_str!("../../../db/002_projection.sql")),
    ("003_blobs", include_str!("../../../db/003_blobs.sql")),
];

const BLOB_CHUNK: usize = 64 * 1024;

type R<T> = Result<T, Box<dyn Error>>;

// ---------------------------------------------------------------------------
// Wire protocol — one JSON request, one JSON response, per connection.
// ---------------------------------------------------------------------------
#[derive(Serialize, Deserialize)]
#[serde(tag = "op")]
enum Request {
    /// Clinical plane: every event at or after this HLC watermark.
    EventsAfter { wall: i64, counter: i32 },
    /// Byte tier: a chunk of a blob's bytes.
    BlobChunk {
        addr_hex: String,
        offset: u64,
        len: u64,
    },
}

#[derive(Serialize, Deserialize)]
struct EventsResponse {
    /// Verbatim signed_bytes, hex-encoded (skeleton simplification; the real
    /// tier ships raw). The receiver reconstructs everything from these bytes.
    events: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct BlobResponse {
    found: bool,
    total_len: u64,
    bytes_hex: String,
}

fn write_frame(s: &mut impl Write, b: &[u8]) -> io::Result<()> {
    s.write_all(&(b.len() as u32).to_be_bytes())?;
    s.write_all(b)?;
    s.flush()
}

fn read_frame(s: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len)?;
    let n = u32::from_be_bytes(len) as usize;
    let mut buf = vec![0u8; n];
    s.read_exact(&mut buf)?;
    Ok(buf)
}

fn request(peer: &str, req: &Request) -> R<Vec<u8>> {
    let mut stream = TcpStream::connect(peer)?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    write_frame(&mut stream, &serde_json::to_vec(req)?)?;
    Ok(read_frame(&mut stream)?)
}

// ---------------------------------------------------------------------------
// Key handling (skeleton: a per-node key file; the registry is ADR-0011).
// ---------------------------------------------------------------------------
fn load_or_create_key(path: &str) -> R<(SigningKey, String)> {
    if let Ok(text) = std::fs::read_to_string(path) {
        let seed: [u8; 32] = hex::decode(text.trim())?
            .try_into()
            .map_err(|_| "key file is not a 32-byte hex seed")?;
        let sk = SigningKey::from_bytes(&seed);
        let kid = hex::encode(sk.verifying_key().to_bytes());
        return Ok((sk, kid));
    }
    let (sk, kid) = cairn_event::generate_key()?;
    std::fs::write(path, hex::encode(sk.to_bytes()))?;
    eprintln!("generated new signing key at {path} (kid {})", &kid[..16]);
    Ok((sk, kid))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

// ---------------------------------------------------------------------------
// Apply: insert a verified event idempotently (set-union) and merge the HLC.
// Shared by `pull`. Verification happens HERE (Bet A2 / the §9 seam); in
// production this gate is an in-DB pgrx function so it cannot be bypassed.
// ---------------------------------------------------------------------------
fn apply_signed(client: &mut postgres::Client, signed_bytes: &[u8]) -> R<bool> {
    let body = verify_self_described(signed_bytes)?; // refuse anything that doesn't verify
    let content_address = cairn_event::event_address(signed_bytes);
    let body_json = serde_json::to_string(&body.payload)?;
    let contributors_json = serde_json::to_string(&body.contributors)?;
    let attachments_json = serde_json::to_string(&body.attachments)?;
    let twin = plaintext_twin(&body);

    let mut tx = client.transaction()?;
    let inserted = tx.execute(
        "INSERT INTO event_log
           (event_id, patient_id, event_type, schema_version, hlc_wall, hlc_counter,
            node_origin, t_effective, signed_bytes, content_address, body, contributors,
            signer_key_id, plaintext_twin, attachments)
         VALUES ($1::text::uuid,$2::text::uuid,$3,$4,$5,$6,$7,$8::text::timestamptz,$9,$10,
                 $11::text::jsonb,$12::text::jsonb,$13,$14,$15::text::jsonb)
         ON CONFLICT DO NOTHING",
        &[
            &body.event_id,
            &body.patient_id,
            &body.event_type,
            &body.schema_version,
            &body.hlc.wall,
            &body.hlc.counter,
            &body.hlc.node_origin,
            &body.t_effective,
            &signed_bytes.to_vec(),
            &content_address,
            &body_json,
            &contributors_json,
            &body.signer_key_id,
            &twin,
            &attachments_json,
        ],
    )?;

    // Learn any attachment references this event carries (reference-eager).
    for att in &body.attachments {
        if let Ok(addr) = hex::decode(&att.digest_hex) {
            tx.execute(
                "SELECT blob_note_reference($1,$2,$3)",
                &[&addr, &att.media_type, &att.byte_len],
            )?;
        }
    }

    // HLC merge: local clock never falls behind an event we have accepted (A3).
    tx.execute(
        "UPDATE hlc_state SET hlc_wall = GREATEST(hlc_wall, $1),
             hlc_counter = CASE WHEN $1 > hlc_wall THEN $2
                                WHEN $1 = hlc_wall THEN GREATEST(hlc_counter, $2)
                                ELSE hlc_counter END
         WHERE id",
        &[&body.hlc.wall, &body.hlc.counter],
    )?;
    tx.commit()?;
    Ok(inserted == 1)
}

// ---------------------------------------------------------------------------
// Subcommands
// ---------------------------------------------------------------------------
fn cmd_init(conn: &str) -> R<()> {
    let mut client = postgres::Client::connect(conn, postgres::NoTls)?;
    for (name, sql) in SCHEMA {
        client.batch_execute(sql)?;
        eprintln!("applied {name}");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_write(
    conn: &str,
    node: &str,
    key_path: &str,
    event_type: &str,
    patient: &str,
    schema_version: &str,
    json_body: &str,
    t_effective: Option<String>,
) -> R<()> {
    let (sk, kid) = load_or_create_key(key_path)?;
    let payload: serde_json::Value = serde_json::from_str(json_body)?;
    let patient_id = if patient == "new" {
        uuid::Uuid::now_v7().to_string()
    } else {
        patient.to_string()
    };

    let mut client = postgres::Client::connect(conn, postgres::NoTls)?;
    let mut tx = client.transaction()?;

    // Advance this node's HLC under a row lock (t_recorded ceiling).
    let row = tx.query_one(
        "SELECT hlc_wall, hlc_counter FROM hlc_state WHERE id FOR UPDATE",
        &[],
    )?;
    let prev_wall: i64 = row.get(0);
    let prev_counter: i32 = row.get(1);
    let phys = now_ms();
    let (wall, counter) = if phys > prev_wall {
        (phys, 0)
    } else {
        (prev_wall, prev_counter + 1)
    };
    tx.execute(
        "UPDATE hlc_state SET hlc_wall=$1, hlc_counter=$2 WHERE id",
        &[&wall, &counter],
    )?;

    let body = EventBody {
        event_id: uuid::Uuid::now_v7().to_string(),
        patient_id: patient_id.clone(),
        event_type: event_type.to_string(),
        schema_version: schema_version.to_string(),
        hlc: Hlc {
            wall,
            counter,
            node_origin: node.to_string(),
        },
        t_effective,
        signer_key_id: kid,
        contributors: serde_json::json!([{ "role": "author", "kind": "human", "node": node }]),
        payload,
        attachments: vec![],
    };

    let signed = sign(&body, &sk)?;
    let body_json = serde_json::to_string(&body.payload)?;
    let contributors_json = serde_json::to_string(&body.contributors)?;
    let twin = plaintext_twin(&body);

    tx.execute(
        "INSERT INTO event_log
           (event_id, patient_id, event_type, schema_version, hlc_wall, hlc_counter,
            node_origin, t_effective, signed_bytes, content_address, body, contributors,
            signer_key_id, plaintext_twin, attachments)
         VALUES ($1::text::uuid,$2::text::uuid,$3,$4,$5,$6,$7,$8::text::timestamptz,$9,$10,
                 $11::text::jsonb,$12::text::jsonb,$13,$14,'[]'::jsonb)",
        &[
            &body.event_id,
            &body.patient_id,
            &body.event_type,
            &body.schema_version,
            &body.hlc.wall,
            &body.hlc.counter,
            &body.hlc.node_origin,
            &body.t_effective,
            &signed.signed_bytes,
            &signed.content_address,
            &body_json,
            &contributors_json,
            &body.signer_key_id,
            &twin,
        ],
    )?;
    tx.commit()?;
    println!(
        "wrote {} {} for patient {} ({} bytes on the clinical plane, addr {})",
        event_type,
        body.event_id,
        patient_id,
        signed.signed_bytes.len(),
        &hex::encode(&signed.content_address)[..16],
    );
    Ok(())
}

fn cmd_put_blob(conn: &str, file: &str, media: &str) -> R<()> {
    let bytes = std::fs::read(file)?;
    let addr = blob_address(&bytes);
    let len = bytes.len() as i64;
    let mut client = postgres::Client::connect(conn, postgres::NoTls)?;
    client.execute(
        "INSERT INTO blob_store (blob_address, media_type, byte_len, content, present, fetched_at)
         VALUES ($1,$2,$3,$4,TRUE,clock_timestamp())
         ON CONFLICT (blob_address) DO UPDATE
            SET content=EXCLUDED.content, present=TRUE, byte_len=EXCLUDED.byte_len,
                fetched_at=clock_timestamp()",
        &[&addr, &media, &len, &bytes],
    )?;
    println!("stored blob {} ({} bytes, {})", hex::encode(&addr), len, media);
    Ok(())
}

fn cmd_pull(conn: &str, peer: &str, peer_name: &str) -> R<()> {
    let mut client = postgres::Client::connect(conn, postgres::NoTls)?;
    client.execute(
        "INSERT INTO sync_state (peer) VALUES ($1) ON CONFLICT (peer) DO NOTHING",
        &[&peer_name],
    )?;
    let wm = client.query_one(
        "SELECT hlc_wall, hlc_counter FROM sync_state WHERE peer=$1",
        &[&peer_name],
    )?;
    let (wall, counter): (i64, i32) = (wm.get(0), wm.get(1));

    let raw = request(
        peer,
        &Request::EventsAfter { wall, counter },
    )?;
    let resp: EventsResponse = serde_json::from_slice(&raw)?;

    let (mut applied, mut max_w, mut max_c) = (0usize, wall, counter);
    for hexed in &resp.events {
        let signed_bytes = hex::decode(hexed)?;
        match apply_signed(&mut client, &signed_bytes) {
            Ok(new) => {
                if new {
                    applied += 1;
                }
                // Track the watermark from whatever verified, new or not.
                if let Ok(b) = verify_self_described(&signed_bytes) {
                    if (b.hlc.wall, b.hlc.counter) > (max_w, max_c) {
                        max_w = b.hlc.wall;
                        max_c = b.hlc.counter;
                    }
                }
            }
            Err(e) => eprintln!("rejected an event from {peer_name}: {e}"), // never poisons the pull
        }
    }
    client.execute(
        "UPDATE sync_state SET hlc_wall=$1, hlc_counter=$2, last_pull_at=clock_timestamp() WHERE peer=$3",
        &[&max_w, &max_c, &peer_name],
    )?;
    println!(
        "pulled from {peer_name}: {} shipped, {} new, watermark -> ({max_w},{max_c})",
        resp.events.len(),
        applied
    );
    Ok(())
}

/// The lazy byte tier (Bet A4): fetch missing blobs in chunks, sleeping a budget
/// between chunks so it is preemptible and cannot saturate the link the clinical
/// plane shares. Crude on purpose — the real tier rate-budgets, this just shows
/// the discipline: byte transfer NEVER blocks clinical sync.
fn cmd_blobd(conn: &str, peer: &str, budget_ms: u64) -> R<()> {
    let mut client = postgres::Client::connect(conn, postgres::NoTls)?;
    let missing = client.query(
        "SELECT encode(blob_address,'hex'), byte_len FROM blob_store WHERE NOT present",
        &[],
    )?;
    for row in missing {
        let addr_hex: String = row.get(0);
        let mut buf: Vec<u8> = Vec::new();
        let mut total: u64 = 0;
        loop {
            let raw = request(
                peer,
                &Request::BlobChunk {
                    addr_hex: addr_hex.clone(),
                    offset: buf.len() as u64,
                    len: BLOB_CHUNK as u64,
                },
            )?;
            let resp: BlobResponse = serde_json::from_slice(&raw)?;
            if !resp.found {
                eprintln!("peer does not have blob {}", &addr_hex[..16]);
                break;
            }
            total = resp.total_len;
            let chunk = hex::decode(&resp.bytes_hex)?;
            if chunk.is_empty() {
                break;
            }
            buf.extend_from_slice(&chunk);
            std::thread::sleep(Duration::from_millis(budget_ms)); // preemptible budget
            if buf.len() as u64 >= total {
                break;
            }
        }
        if buf.len() as u64 != total {
            continue; // resumable: try again next pass, verifying what arrived
        }
        // Content-verify before storing (§4.4 — a wrong-hash blob is never served).
        let got = blob_address(&buf);
        if hex::encode(&got) != addr_hex {
            eprintln!("blob {} failed BLAKE3 verification — discarded", &addr_hex[..16]);
            continue;
        }
        client.execute(
            "UPDATE blob_store SET content=$1, present=TRUE, byte_len=$2, fetched_at=clock_timestamp()
             WHERE blob_address=$3",
            &[&buf, &(buf.len() as i64), &got],
        )?;
        println!("fetched blob {} ({} bytes, verified)", &addr_hex[..16], buf.len());
    }
    Ok(())
}

fn cmd_serve(conn: String, listen: &str) -> R<()> {
    let listener = TcpListener::bind(listen)?;
    eprintln!("serving on {listen}");
    for stream in listener.incoming() {
        let stream = stream?;
        let conn = conn.clone();
        std::thread::spawn(move || {
            if let Err(e) = serve_conn(&conn, stream) {
                eprintln!("connection error: {e}");
            }
        });
    }
    Ok(())
}

fn serve_conn(conn: &str, mut stream: TcpStream) -> R<()> {
    let mut client = postgres::Client::connect(conn, postgres::NoTls)?;
    let raw = read_frame(&mut stream)?;
    let req: Request = serde_json::from_slice(&raw)?;
    let resp: Vec<u8> = match req {
        Request::EventsAfter { wall, counter } => {
            let rows = client.query(
                "SELECT encode(signed_bytes,'hex') FROM event_log
                 WHERE (hlc_wall, hlc_counter) >= ($1,$2)
                 ORDER BY hlc_wall, hlc_counter, node_origin",
                &[&wall, &counter],
            )?;
            let events = rows.iter().map(|r| r.get::<_, String>(0)).collect();
            serde_json::to_vec(&EventsResponse { events })?
        }
        Request::BlobChunk {
            addr_hex,
            offset,
            len,
        } => {
            let addr = hex::decode(&addr_hex)?;
            // substring/octet_length take int4; bind the chunk window as i32.
            // (Skeleton limit: blobs > 2 GiB need bigint offsets — noted in the README.)
            let row = client.query_opt(
                "SELECT encode(substring(content from $2 for $3),'hex'), octet_length(content)
                 FROM blob_store WHERE blob_address=$1 AND present",
                &[&addr, &(offset as i32 + 1), &(len as i32)],
            )?;
            let resp = match row {
                Some(r) => BlobResponse {
                    found: true,
                    total_len: r.get::<_, i32>(1) as u64,
                    bytes_hex: r.get(0),
                },
                None => BlobResponse {
                    found: false,
                    total_len: 0,
                    bytes_hex: String::new(),
                },
            };
            serde_json::to_vec(&resp)?
        }
    };
    write_frame(&mut stream, &resp)?;
    Ok(())
}

// ---------------------------------------------------------------------------
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn usage() -> ! {
    eprintln!(
        "cairn-sync — Cairn walking skeleton (Spike 0001)

USAGE (all take --conn <postgres-uri>):
  init     --conn URI
  write    --conn URI --node NAME --key PATH --type T --patient (UUID|new)
           --schema SV --json '<body>' [--effective ISO8601]
  put-blob --conn URI --file PATH --media MEDIA_TYPE
  pull     --conn URI --peer HOST:PORT --peer-name NAME
  blobd    --conn URI --peer HOST:PORT [--budget-ms N]
  serve    --conn URI --listen HOST:PORT

Run over WireGuard; NoTls is intentional (the link is the transport)."
    );
    std::process::exit(2)
}

fn main() -> R<()> {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("");
    let conn = flag(&args, "--conn");
    let need = |o: Option<String>| o.unwrap_or_else(|| usage());

    match cmd {
        "init" => cmd_init(&need(conn))?,
        "write" => cmd_write(
            &need(conn),
            &need(flag(&args, "--node")),
            &flag(&args, "--key").unwrap_or_else(|| "node.key".into()),
            &need(flag(&args, "--type")),
            &need(flag(&args, "--patient")),
            &flag(&args, "--schema").unwrap_or_else(|| "v1".into()),
            &need(flag(&args, "--json")),
            flag(&args, "--effective"),
        )?,
        "put-blob" => cmd_put_blob(
            &need(conn),
            &need(flag(&args, "--file")),
            &need(flag(&args, "--media")),
        )?,
        "pull" => cmd_pull(
            &need(conn),
            &need(flag(&args, "--peer")),
            &need(flag(&args, "--peer-name")),
        )?,
        "blobd" => cmd_blobd(
            &need(conn),
            &need(flag(&args, "--peer")),
            flag(&args, "--budget-ms")
                .and_then(|s| s.parse().ok())
                .unwrap_or(50),
        )?,
        "serve" => cmd_serve(need(conn), &need(flag(&args, "--listen")))?,
        _ => usage(),
    }
    Ok(())
}
