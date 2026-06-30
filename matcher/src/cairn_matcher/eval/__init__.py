"""Cairn matcher eval harness — measurement substrate for the §5.2 advisory matcher.

Pure by default (stdlib only): dataset format, scorer/banding metrics, and a CLI. The
blocking-recall layer (`blocking_eval`) is the one DB-touching module and needs the
optional `pipeline` extra (psycopg). This package ships NO clinical floor and makes NO
link decision — a defect yields a wrong metric a human reads, never record corruption.
"""
