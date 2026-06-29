"""IO-bearing matcher pipeline (piece B2).

This sub-package is the advisory pipeline that connects B1's pure scoring core to a
node's projections and persists a proposal. It is deliberately SEPARATE from the pure
core: `adapter` and `banding` are pure (no psycopg), while `db` and `runner` are the
only modules that touch Postgres. Importing `db`/`runner` requires the optional
`pipeline` extra (psycopg); `adapter`/`banding` never do.
"""
