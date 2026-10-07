"""Remote read-only worker; stdout is exclusively UTF-8 CSV."""
import hashlib
import json
import os
import sys
import time

PREVIEW_CELL_CHARS = 1024
PREVIEW_MAX_COLUMNS = 256
PREVIEW_MAX_BYTES = 4 * 1024 * 1024


def preview_query(sql, table, predicate, names):
    if len(names) > PREVIEW_MAX_COLUMNS:
        raise ValueError("preview has too many columns")
    # Truncate inside PostgreSQL, before values reach libpq/Python or the client.
    projection = sql.SQL(",").join(
        sql.SQL("CASE WHEN length({0}::text) > {1} THEN left({0}::text, {1}) || '…' ELSE {0}::text END AS {0}").format(
            sql.Identifier(name), sql.Literal(PREVIEW_CELL_CHARS)) for name in names)
    return sql.SQL("SELECT {} FROM {}{} LIMIT 10").format(projection, table, predicate)


def preview_json(names, rows):
    encoded = json.dumps({"columns": names, "rows": rows, "cell_character_limit": PREVIEW_CELL_CHARS}, ensure_ascii=True)
    if len(encoded) > PREVIEW_MAX_BYTES:
        raise ValueError("preview response exceeds size limit")
    return encoded


def emit(value):
    sys.stderr.write("PGCSV " + json.dumps(value, ensure_ascii=True) + "\n")
    sys.stderr.flush()


def run():
    import psycopg
    from psycopg import sql

    config = json.load(sys.stdin)
    if config.get("action") == "ssh":
        sys.stdout.write(json.dumps({"connected": True, "psycopg": psycopg.__version__}))
        return
    if config.get("start") or config.get("end"):
        from datetime import datetime
        start = datetime.fromisoformat(config["start"]) if config.get("start") else None
        end = datetime.fromisoformat(config["end"]) if config.get("end") else None
        if start and end and start >= end:
            raise ValueError("start must be earlier than end")
    dsn = os.environ.get(config.get("database_env", "DATABASE_URL"))
    if config.get("env_file"):
        # Read a literal KEY=value setting; never execute shell content.
        with open(os.path.expanduser(config["env_file"]), encoding="utf-8") as source:
            for line in source:
                key, sep, value = line.strip().partition("=")
                if sep and key == config.get("database_env", "DATABASE_URL"):
                    dsn = value.strip()
                    if dsn[:1] in ("'", '"') and dsn[-1:] == dsn[:1]:
                        dsn = dsn[1:-1]
    db = config.get("database")
    if not dsn and not db:
        raise ValueError("database configuration missing")
    params = db or {}
    if config.get("database_name"):
        params = dict(params, dbname=config["database_name"])
    with psycopg.connect(dsn or "", connect_timeout=15, **params) as conn:
        conn.execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
        conn.execute("SELECT set_config('statement_timeout', %s, true)", (str(config.get("timeout_ms", 300000)),))
        conn.execute("SELECT set_config('TimeZone', %s, true)", (config.get("timezone", "Asia/Tokyo"),))
        conn.execute("SET LOCAL DateStyle TO 'ISO, YMD'")
        if config.get("action") == "databases":
            rows = conn.execute("SELECT datname FROM pg_database WHERE datallowconn AND has_database_privilege(oid, 'CONNECT') ORDER BY datname").fetchall()
            sys.stdout.write(json.dumps({"names": [r[0] for r in rows], "current": conn.execute("SELECT current_database()").fetchone()[0]}))
            conn.rollback()
            return
        if config.get("action") == "list":
            rows = conn.execute("SELECT table_schema,table_name,column_name,data_type FROM information_schema.columns WHERE table_schema NOT IN ('pg_catalog','information_schema') ORDER BY table_schema,table_name,ordinal_position").fetchall()
            sys.stdout.write(json.dumps(rows, ensure_ascii=True))
            conn.rollback()
            return
        table = sql.Identifier(config.get("schema", "public"), config["table"])
        columns = config.get("columns")
        projection = sql.SQL(",").join(map(sql.Identifier, columns)) if columns else sql.SQL("*")
        conditions = []
        for bound, op in (("start", ">="), ("end", "<")):
            if config.get(bound):
                if not config.get("time_column"):
                    raise ValueError("time_column missing")
                conditions.append(sql.SQL("{} " + op + " {}").format(sql.Identifier(config["time_column"]), sql.Literal(config[bound])))
        predicate = sql.SQL(" WHERE ") + sql.SQL(" AND ").join(conditions) if conditions else sql.SQL("")
        query = sql.SQL("SELECT {} FROM {}{}").format(projection, table, predicate)
        if config.get("action") == "preview":
            names = [c.name for c in conn.execute(query + sql.SQL(" LIMIT 0")).description]
            cursor = conn.execute(preview_query(sql, table, predicate, names))
            values = cursor.fetchall()
            sys.stdout.write(preview_json(names, values))
            conn.rollback()
            return
        count = conn.execute(sql.SQL("SELECT count(*) FROM {}{}").format(table, predicate)).fetchone()[0]
        header = conn.execute(query + sql.SQL(" LIMIT 0")).description
        names = [col.name for col in header]
        if config.get("action") == "estimate":
            sample_size=0
            with conn.cursor().copy(sql.SQL("COPY ({} LIMIT 1000) TO STDOUT WITH (FORMAT CSV, HEADER, ENCODING 'UTF8')").format(query)) as copy:
                for block in copy: sample_size+=len(block)
            estimated=int(sample_size * count / max(1,min(count,1000))) + 3 if count else sample_size+3
            sys.stdout.write(json.dumps({"rows":count,"estimated_bytes":estimated,"sample_rows":min(count,1000)}))
            conn.rollback()
            return
        emit({"event": "start", "rows": count, "columns": names, "snapshot": conn.execute("SELECT CURRENT_TIMESTAMP").fetchone()[0].isoformat()})
        started = time.monotonic()
        digest = hashlib.sha256()
        size = 0
        with conn.cursor().copy(sql.SQL("COPY ({}) TO STDOUT WITH (FORMAT CSV, HEADER, ENCODING 'UTF8')").format(query)) as copy:
            for block in copy:
                digest.update(block)
                size += len(block)
                sys.stdout.buffer.write(block)
        sys.stdout.buffer.flush()
        conn.rollback()
        emit({"event": "complete", "rows": count, "bytes": size, "sha256": digest.hexdigest(), "stream_seconds": time.monotonic() - started})


if __name__ == "__main__":
    try:
        run()
    except Exception as exc:
        # Exceptions can contain DSNs/data; only expose type and SQLSTATE.
        emit({"event": "error", "type": type(exc).__name__, "sqlstate": getattr(exc, "sqlstate", None)})
        sys.exit(1)
