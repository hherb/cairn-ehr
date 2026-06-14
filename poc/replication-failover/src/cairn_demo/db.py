"""Connection helpers for the two demo nodes.

Node A and Node B may live on one machine (development default) or on two
separate machines reached across the network (the "pull the cable" demo). The
topology is decided by :mod:`cairn_demo.config`; everything else here is
identical in both cases.
"""

from __future__ import annotations

import os
from dataclasses import dataclass

import psycopg

from . import config


@dataclass(frozen=True)
class Node:
    """A demo node: a name and how to reach its PostgreSQL cluster."""

    name: str          # "A" or "B"
    host: str
    port: int
    dbname: str
    user: str
    password: str | None = None
    is_self: bool = False   # True if this node runs on the local machine

    @property
    def location(self) -> str:
        return "this machine" if self.is_self else f"{self.host}:{self.port}"

    @property
    def dsn(self) -> str:
        parts = [
            f"host={self.host}",
            f"port={self.port}",
            f"dbname={self.dbname}",
            f"user={self.user}",
            # Short timeout so an unplugged peer is reported OFFLINE quickly
            # instead of hanging the dashboard.
            "connect_timeout=2",
        ]
        if self.password:
            parts.append(f"password={self.password}")
        return " ".join(parts)

    def connect(self) -> psycopg.Connection:
        """Open a connection, raising psycopg.OperationalError if unreachable."""
        return psycopg.connect(self.dsn)

    def connect_autocommit(self) -> psycopg.Connection:
        """Open an autocommit connection (convenience for one-shot writes)."""
        conn = psycopg.connect(self.dsn)
        conn.autocommit = True
        return conn

    def is_up(self) -> bool:
        """True if the node is currently reachable and accepting connections."""
        try:
            with psycopg.connect(self.dsn) as conn:
                conn.execute("SELECT 1")
            return True
        except psycopg.OperationalError:
            return False


def _build_nodes() -> dict[str, Node]:
    dbname = config.get("CAIRN_DB_NAME", "cairn")
    user = config.get("CAIRN_DB_USER", os.environ.get("USER", "postgres"))
    password = config.get("CAIRN_DB_PASSWORD")  # None unless networked w/ auth

    if config.is_networked():
        # Two-machine topology: one local node + one remote peer.
        self_name = config.self_name()
        self_port = int(config.get("CAIRN_SELF_PORT", "55432"))
        peer_name = (config.get("CAIRN_PEER_NAME")
                     or ("B" if self_name == "A" else "A")).strip().upper()
        peer_host = config.get("CAIRN_PEER_HOST", "127.0.0.1")
        peer_port = int(config.get("CAIRN_PEER_PORT", "55432"))
        nodes = {
            self_name: Node(self_name, "127.0.0.1", self_port, dbname, user,
                            password, is_self=True),
            peer_name: Node(peer_name, peer_host, peer_port, dbname, user,
                            password, is_self=False),
        }
        return nodes

    # Single-machine default: two local nodes on adjacent ports.
    return {
        "A": Node("A", "127.0.0.1", int(os.environ.get("NODE_A_PORT", "55432")),
                  dbname, user, password, is_self=True),
        "B": Node("B", "127.0.0.1", int(os.environ.get("NODE_B_PORT", "55433")),
                  dbname, user, password, is_self=True),
    }


NODES: dict[str, Node] = _build_nodes()


def get_node(name: str) -> Node:
    key = name.strip().upper()
    if key not in NODES:
        raise ValueError(f"unknown node {name!r}; expected one of {sorted(NODES)}")
    return NODES[key]
