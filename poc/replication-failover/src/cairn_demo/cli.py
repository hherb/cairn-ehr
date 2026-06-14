"""Command-line interface for the Cairn replication/failover PoC.

Run ``uv run cairn-demo --help`` for the command list. The headline commands
for a live demo are:

    uv run cairn-demo dashboard      # live side-by-side view of both nodes
    uv run cairn-demo patient add "Jane Doe" --dob 1980-04-12 --sex F
    uv run cairn-demo note add Jane "Seen in ED, chest pain, ECG normal."
    uv run cairn-demo sync           # heal after a partition
    uv run cairn-demo walkthrough    # fully scripted, narrated rehearsal
"""

from __future__ import annotations

import time
from typing import Optional

import psycopg
import typer
from rich.align import Align
from rich.console import Console, Group
from rich.live import Live
from rich.panel import Panel
from rich.table import Table
from rich.text import Text

from . import config
from . import events as ev
from . import projections as proj
from .db import NODES, Node, get_node
from .sync import sync_pair

# The node this machine writes to by default ("A" single-machine, or whichever
# node CAIRN_SELF_NAME names in a two-machine setup).
SELF = config.self_name()

app = typer.Typer(
    add_completion=False,
    help="Cairn EHR — offline-first replication & failover proof-of-concept.",
    no_args_is_help=True,
)
patient_app = typer.Typer(help="Create and inspect patients.", no_args_is_help=True)
note_app = typer.Typer(help="Add and inspect clinical notes.", no_args_is_help=True)
app.add_typer(patient_app, name="patient")
app.add_typer(note_app, name="note")

console = Console()

NODE_COLORS = {"A": "cyan", "B": "magenta"}


# --------------------------------------------------------------------------- #
# helpers
# --------------------------------------------------------------------------- #
def _open(node: Node) -> psycopg.Connection:
    try:
        conn = node.connect()
        conn.autocommit = True
        return conn
    except psycopg.OperationalError as exc:
        console.print(
            f"[bold red]Node {node.name} is OFFLINE[/] ({node.location}) — "
            f"the plug is pulled.\n[dim]{str(exc).strip().splitlines()[0]}[/]"
        )
        raise typer.Exit(code=2)


def _resolve_patient(conn: psycopg.Connection, query: str) -> dict:
    """Find one patient by case-insensitive name substring or UUID prefix."""
    matches = [
        p for p in proj.patients(conn)
        if query.lower() in (p["name"] or "").lower()
        or p["patient_id"].startswith(query.lower())
    ]
    if not matches:
        console.print(f"[red]No patient matches {query!r} on this node.[/]")
        raise typer.Exit(code=1)
    if len(matches) > 1:
        names = ", ".join(m["name"] for m in matches)
        console.print(f"[red]{query!r} is ambiguous: {names}. Be more specific.[/]")
        raise typer.Exit(code=1)
    return matches[0]


# --------------------------------------------------------------------------- #
# patient commands
# --------------------------------------------------------------------------- #
@patient_app.command("add")
def patient_add(
    name: str = typer.Argument(..., help="Patient full name."),
    dob: Optional[str] = typer.Option(None, help="Date of birth, e.g. 1980-04-12."),
    sex: Optional[str] = typer.Option(None, help="Sex, free text (e.g. F, M, X)."),
    node: str = typer.Option(SELF, "--node", "-n", help="Which node to write to (A or B)."),
):
    """Register a new patient on a node."""
    n = get_node(node)
    conn = _open(n)
    pid = ev.create_patient(conn, node_origin=n.name, name=name, dob=dob, sex=sex)
    console.print(
        f"[green]✓[/] Created patient [bold]{name}[/] on node "
        f"[{NODE_COLORS[n.name]}]{n.name}[/]  [dim]{pid}[/]"
    )


@patient_app.command("list")
def patient_list(
    node: str = typer.Option(SELF, "--node", "-n", help="Which node to read (A or B)."),
):
    """List current patients on a node."""
    n = get_node(node)
    conn = _open(n)
    rows = proj.patients(conn)
    if not rows:
        console.print(f"[dim]No patients on node {n.name} yet.[/]")
        return
    table = Table(title=f"Patients on node {n.name}")
    table.add_column("Name"); table.add_column("DOB"); table.add_column("Sex")
    table.add_column("Origin"); table.add_column("UUID", style="dim")
    for p in rows:
        table.add_row(p["name"], p["dob"] or "—", p["sex"] or "—",
                      p["node_origin"], p["patient_id"])
    console.print(table)


# --------------------------------------------------------------------------- #
# note commands
# --------------------------------------------------------------------------- #
@note_app.command("add")
def note_add(
    patient: str = typer.Argument(..., help="Patient name substring or UUID prefix."),
    text: str = typer.Argument(..., help="The clinical note text."),
    node: str = typer.Option(SELF, "--node", "-n", help="Which node to write to (A or B)."),
):
    """Append a clinical note (the atomic health-record component) to a patient."""
    n = get_node(node)
    conn = _open(n)
    p = _resolve_patient(conn, patient)
    ev.add_note(conn, node_origin=n.name, patient_id=p["patient_id"], text=text)
    console.print(
        f"[green]✓[/] Note added for [bold]{p['name']}[/] on node "
        f"[{NODE_COLORS[n.name]}]{n.name}[/]"
    )


@note_app.command("list")
def note_list(
    node: str = typer.Option(SELF, "--node", "-n", help="Which node to read (A or B)."),
):
    """List clinical notes on a node, in causal (HLC) order."""
    n = get_node(node)
    conn = _open(n)
    rows = proj.notes(conn)
    if not rows:
        console.print(f"[dim]No notes on node {n.name} yet.[/]")
        return
    table = Table(title=f"Notes on node {n.name} (causal order)")
    table.add_column("#", style="dim"); table.add_column("Patient")
    table.add_column("Note"); table.add_column("Origin")
    for i, r in enumerate(rows, 1):
        table.add_row(str(i), r["patient_name"] or "—", r["text"], r["node_origin"])
    console.print(table)


# --------------------------------------------------------------------------- #
# sync
# --------------------------------------------------------------------------- #
@app.command()
def sync(
    watch: bool = typer.Option(
        False, "--watch", "-w",
        help="Keep syncing every few seconds — auto-heals the moment a node returns.",
    ),
    interval: float = typer.Option(2.0, help="Seconds between syncs in --watch mode."),
):
    """Synchronise nodes A and B (conflict-free set-union). Safe to re-run."""
    def once() -> None:
        a, b = get_node("A"), get_node("B")
        try:
            with a.connect() as ca, b.connect() as cb:
                ca.autocommit = True
                cb.autocommit = True
                res = sync_pair(ca, cb)
        except psycopg.OperationalError:
            down = "A" if not a.is_up() else "B"
            console.print(f"[yellow]Cannot sync: node {down} is offline. Waiting…[/]")
            return
        if res.total_copied == 0 and res.converged:
            console.print("[green]✓ Already in sync.[/] Nothing to do.")
        else:
            console.print(
                f"[green]✓ Synced.[/] A→B: [bold]{res.a_to_b}[/]  "
                f"B→A: [bold]{res.b_to_a}[/]  "
                + ("[green]converged ✓[/]" if res.converged else "[red]NOT converged[/]")
            )

    if not watch:
        once()
        return
    console.print("[dim]Watching — Ctrl-C to stop. Pull a plug and watch it heal.[/]")
    try:
        while True:
            once()
            time.sleep(interval)
    except KeyboardInterrupt:
        console.print("\n[dim]Stopped watching.[/]")


# --------------------------------------------------------------------------- #
# status (one-shot) + dashboard (live)
# --------------------------------------------------------------------------- #
def _node_title(node: Node) -> str:
    color = NODE_COLORS[node.name]
    if config.is_networked():
        # Two machines: distinguish the local node from the remote peer.
        where = "this machine" if node.is_self else f"{node.host}:{node.port}"
        marker = "  ◀ you are here" if node.is_self else ""
    else:
        # One machine: both nodes are local; tell them apart by port.
        where = f":{node.port}"
        marker = ""
    return f"[{color}]NODE {node.name}[/]  [dim]{where}[/]{marker}"


def _node_panel(node: Node) -> Panel:
    color = NODE_COLORS[node.name]
    try:
        with node.connect() as conn:
            conn.autocommit = True
            count = proj.event_count(conn)
            patients = proj.patients(conn)
            notes = proj.notes(conn)
    except psycopg.OperationalError:
        reason = ("(this machine's database is down)" if node.is_self
                  else "(cable pulled / peer unreachable)")
        body = Align.center(
            Text(f"\n⏻  OFFLINE\n{reason}\n", style="bold red"),
            vertical="middle",
        )
        return Panel(body, title=_node_title(node), border_style="red", height=20)

    inner = Table.grid(padding=(0, 1))
    inner.add_row(Text(f"● ONLINE   {count} events in log", style="green"))
    inner.add_row(Text(""))
    inner.add_row(Text("Patients", style="bold underline"))
    if patients:
        for p in patients:
            tag = "✎" if p["last_event"] == "patient.amended" else " "
            inner.add_row(Text(f"  {tag} {p['name']}  ({p['sex'] or '?'}, {p['dob'] or '?'})"
                               f"  [from {p['node_origin']}]"))
    else:
        inner.add_row(Text("  —", style="dim"))
    inner.add_row(Text(""))
    inner.add_row(Text("Notes", style="bold underline"))
    if notes:
        for r in notes:
            origin = r["node_origin"]
            badge_style = NODE_COLORS.get(origin, "white")
            line = Text("  • ")
            line.append(f"[{origin}] ", style=badge_style)
            line.append(f"{r['patient_name'] or '?'}: {r['text']}")
            inner.add_row(line)
    else:
        inner.add_row(Text("  —", style="dim"))

    return Panel(inner, title=_node_title(node), border_style=color, height=20)


def _convergence_banner() -> Panel:
    a, b = get_node("A"), get_node("B")
    try:
        with a.connect() as ca, b.connect() as cb:
            ca.autocommit = True; cb.autocommit = True
            ids_a = proj.event_ids(ca)
            ids_b = proj.event_ids(cb)
    except psycopg.OperationalError:
        down = "A" if not a.is_up() else "B"
        return Panel(Align.center(Text(
            f"PARTITIONED — node {down} is unreachable. The surviving node keeps "
            f"working; data written now will reconcile after reconnection.",
            style="bold yellow")), border_style="yellow")
    if ids_a == ids_b:
        return Panel(Align.center(Text(
            f"IN SYNC ✓   both nodes hold the identical {len(ids_a)} events",
            style="bold green")), border_style="green")
    pending = len(ids_a ^ ids_b)
    return Panel(Align.center(Text(
        f"DIVERGED — {pending} event(s) not yet replicated.  "
        f"Run  'cairn-demo sync'  to heal.",
        style="bold red")), border_style="red")


def _dashboard_render() -> Group:
    cols = Table.grid(expand=True)
    cols.add_column(ratio=1); cols.add_column(ratio=1)
    cols.add_row(_node_panel(get_node("A")), _node_panel(get_node("B")))
    title = Align.center(Text("CAIRN — replication & failover", style="bold"))
    return Group(title, cols, _convergence_banner())


@app.command()
def dashboard(
    refresh: float = typer.Option(1.2, help="Seconds between refreshes."),
):
    """Live side-by-side view of both nodes. The visual centrepiece of the demo."""
    try:
        with Live(_dashboard_render(), console=console, refresh_per_second=4,
                  screen=True) as live:
            while True:
                time.sleep(refresh)
                live.update(_dashboard_render())
    except KeyboardInterrupt:
        pass
    console.print("[dim]Dashboard closed.[/]")


@app.command()
def status():
    """One-shot status of both nodes and whether they have converged."""
    console.print(_dashboard_render())


# --------------------------------------------------------------------------- #
# scripted walkthrough (rehearsal / fallback)
# --------------------------------------------------------------------------- #
@app.command()
def walkthrough(
    auto: bool = typer.Option(
        False, "--auto", help="Run without pausing for Enter between steps."),
    reset: bool = typer.Option(
        True, help="Start from an empty, in-sync slate (truncates both nodes)."),
):
    """Scripted, narrated run of the headline scenario: BOTH clinics keep working
    during a network split, and the records reconcile losslessly afterwards.

    This proves the convergence *math* at the data layer (it simulates the split
    by withholding sync, so it always works for rehearsal). The visceral live
    version — physically pulling a cable — is in RUNBOOK.md / TWO-MACHINE-RUNBOOK.md.
    """
    def step(msg: str) -> None:
        console.rule(f"[bold]{msg}")
    def narrate(msg: str) -> None:
        console.print(Text(msg, style="italic"))
    def pause(prompt: str) -> None:
        if auto:
            time.sleep(1.2)
        else:
            typer.prompt(f"\n>>> {prompt} [Enter]", default="", show_default=False)

    a, b = get_node("A"), get_node("B")
    if not (a.is_up() and b.is_up()):
        console.print("[red]Both nodes must be up to start. Run setup first.[/]")
        raise typer.Exit(1)

    if reset:
        for n in (a, b):
            with n.connect_autocommit() as c:
                c.execute("TRUNCATE event_log")
                c.execute("UPDATE hlc_state SET hlc_wall=0, hlc_counter=0 WHERE id IS TRUE")

    step("1. Two independent clinics, both online and in sync")
    narrate("Node A and Node B are separate PostgreSQL servers — two clinics, "
            "or a clinic and the regional centre. Right now they agree on everything.")
    status()
    pause("Begin")

    step("2. A patient is registered at clinic A, then the clinics sync")
    pid = ev.create_patient(a.connect_autocommit(), node_origin="A",
                            name="Walkthrough Patient", dob="1972-09-30", sex="F")
    with a.connect() as ca, b.connect() as cb:
        ca.autocommit = True; cb.autocommit = True
        sync_pair(ca, cb)
    narrate("Set-union sync — conflict-free. Both clinics now know the patient.")
    status()
    pause("Now the network link between them goes down")

    step("3. THE LINK GOES DOWN — and BOTH clinics keep working")
    narrate("This is the heart of it: neither clinic is blocked, neither waits "
            "for the other. Each writes to the same patient, independently.")
    with a.connect_autocommit() as ca:
        ev.add_note(ca, node_origin="A", patient_id=pid,
                    text="Clinic A during the split: started IV fluids.")
    with b.connect_autocommit() as cb:
        ev.add_note(cb, node_origin="B", patient_id=pid,
                    text="Clinic B during the split: gave analgesia.")
    narrate("Each clinic sees only its OWN note — the records have diverged.")
    status()
    console.print(Text("  Clinic A's view:", style="bold cyan"))
    note_list(node="A")
    console.print(Text("  Clinic B's view:", style="bold magenta"))
    note_list(node="B")
    pause("Now the link is restored")

    step("4. THE LINK IS RESTORED — the records reconcile themselves")
    with a.connect() as ca, b.connect() as cb:
        ca.autocommit = True; cb.autocommit = True
        res = sync_pair(ca, cb)
    narrate(f"Set-union sync: A→B {res.a_to_b}, B→A {res.b_to_a}. "
            "No 'winner' was chosen, nothing was overwritten, nothing was lost.")
    status()

    step("5. Both clinics now hold BOTH notes — in the identical order")
    console.print(Text("  Clinic A's view:", style="bold cyan"))
    note_list(node="A")
    console.print(Text("  Clinic B's view:", style="bold magenta"))
    note_list(node="B")
    narrate("Same two events, same causal order, on both machines — guaranteed by "
            "the Hybrid Logical Clock, not by luck.")
    console.rule("[bold green]Converged, losslessly. That is the proof.")


if __name__ == "__main__":
    app()
