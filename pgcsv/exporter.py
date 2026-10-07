"""Local byte-preserving OpenSSH transport and atomic CSV publication."""
import argparse
import base64
import csv
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import threading
import time


class ExportError(RuntimeError):
    pass


def command(profile):
    host = profile["ssh_host"]
    if not re.fullmatch(r"[A-Za-z0-9_.@:-]+", host) or host.startswith("-"):
        raise ValueError("invalid SSH host")
    worker = Path(__file__).with_name("remote.py").read_bytes()
    code = "import base64;exec(base64.b64decode(" + repr(base64.b64encode(worker).decode()) + "))"
    executable = profile["remote_python"]
    if not executable.startswith("/") or "\n" in executable:
        raise ValueError("remote_python must be an absolute Linux path")
    return ["ssh", "-T", "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=yes", "-o", "ConnectTimeout=15", host,
            shlex.quote(executable) + " -c " + shlex.quote(code)]


def validate_config(profile):
    forbidden = {"password", "dsn", "database_url", "private_key"}
    if forbidden.intersection(profile):
        raise ValueError("secrets must remain on the remote server")
    if profile.get("start") or profile.get("end"):
        from datetime import datetime
        for key in ("start", "end"):
            if profile.get(key):
                datetime.fromisoformat(profile[key])
        if profile.get("start") and profile.get("end"):
            if datetime.fromisoformat(profile["start"]) >= datetime.fromisoformat(profile["end"]):
                raise ValueError("start must be earlier than end")
        if not profile.get("time_column"):
            raise ValueError("time_column is required")
    if profile.get("columns") is not None and (not profile["columns"] or len(set(profile["columns"])) != len(profile["columns"])):
        raise ValueError("columns must be nonempty and unique")


def export(profile, output, overwrite=False, cancel=None):
    validate_config(profile)
    argv = command(profile)
    output = Path(output).absolute()
    if output.exists() and not overwrite:
        raise FileExistsError(output)
    if not output.parent.is_dir():
        raise ExportError("destination directory missing")
    if shutil.disk_usage(output.parent).free < 1024 * 1024:
        raise ExportError("insufficient free space")
    started = time.monotonic()
    events, diagnostics = [], []
    proc = None
    temporary = None
    stop = threading.Event()
    watcher = None
    try:
        fd, temporary = tempfile.mkstemp(prefix=output.name + ".", suffix=".part", dir=output.parent)
        with os.fdopen(fd, "wb", buffering=1024 * 1024) as target:
            target.write(b"\xef\xbb\xbf")
            proc = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            def read_stderr():
                for raw in proc.stderr:
                    line = raw.decode("utf-8", errors="replace").strip()
                    if line.startswith("PGCSV "):
                        try:
                            events.append(json.loads(line[6:]))
                        except ValueError:
                            diagnostics.append("invalid worker metadata")
                    elif len(diagnostics) < 10:
                        diagnostics.append(line[:500])
            reader = threading.Thread(target=read_stderr, daemon=True)
            reader.start()
            def watch_cancel():
                while not stop.wait(0.05):
                    if cancel is not None and cancel.is_set():
                        proc.terminate()
                        return
            watcher = threading.Thread(target=watch_cancel, daemon=True)
            watcher.start()
            payload = {key: value for key, value in profile.items() if key not in ("ssh_host", "remote_python")}
            proc.stdin.write(json.dumps(payload).encode("utf-8"))
            proc.stdin.close()
            digest, size = hashlib.sha256(), 0
            while True:
                block = proc.stdout.read(1024 * 1024)
                if not block:
                    break
                target.write(block)
                digest.update(block)
                size += len(block)
            exit_code = proc.wait()
            stop.set()
            watcher.join(timeout=1)
            reader.join(timeout=5)
            if cancel is not None and cancel.is_set():
                raise ExportError("cancelled")
            completed = [e for e in events if e.get("event") == "complete"]
            if exit_code or len(completed) != 1 or any(e.get("event") == "error" for e in events):
                errors = [e for e in events if e.get("event") == "error"]
                raise ExportError(json.dumps({"exit_code": exit_code, "errors": errors, "ssh": diagnostics}, ensure_ascii=False))
            metadata = completed[0]
            if metadata["bytes"] != size or metadata["sha256"] != digest.hexdigest():
                raise ExportError("stream integrity mismatch")
            target.flush()
            os.fsync(target.fileno())
        stream_seconds = time.monotonic() - started
        csv.field_size_limit(16 * 1024 * 1024)
        with open(temporary, newline="", encoding="utf-8-sig") as source:
            rows = csv.reader(source, strict=True)
            header = next(rows)
            beginnings = [e for e in events if e.get("event") == "start"]
            if len(beginnings) != 1 or header != beginnings[0]["columns"]:
                raise ExportError("header mismatch")
            count = 0
            for row in rows:
                if cancel is not None and cancel.is_set():
                    raise ExportError("cancelled")
                if len(row) != len(header):
                    raise ExportError("column count mismatch")
                count += 1
            if count != metadata["rows"]:
                raise ExportError("row count mismatch")
        if output.exists() and not overwrite:
            raise FileExistsError(output)
        if overwrite:
            os.replace(temporary, output)
        else:
            # Hard-link publication refuses races, including on NTFS.
            os.link(temporary, output)
            os.unlink(temporary)
        temporary = None
        return {"rows": count, "bytes": size + 3, "sha256_without_bom": digest.hexdigest(),
                "transfer_and_save_seconds": stream_seconds, "total_seconds": time.monotonic() - started,
                "remote_stream_seconds": metadata["stream_seconds"], "snapshot": beginnings[0]["snapshot"], "output": str(output)}
    finally:
        stop.set()
        if proc:
            if proc.poll() is None:
                proc.kill()
            proc.wait()
            for pipe in (proc.stdin, proc.stdout, proc.stderr):
                if pipe and not pipe.closed:
                    pipe.close()
        if watcher:
            watcher.join(timeout=1)
        if temporary and os.path.exists(temporary):
            os.unlink(temporary)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("profile", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--overwrite", action="store_true")
    parser.add_argument("--list", action="store_true", help="list remote table columns")
    args = parser.parse_args()
    for pipe in (sys.stdout, sys.stderr):
        if hasattr(pipe, "reconfigure"):
            pipe.reconfigure(encoding="utf-8")
    cancel = threading.Event()
    import signal
    signal.signal(signal.SIGINT, lambda *_: cancel.set())
    try:
        profile = json.loads(args.profile.read_text(encoding="utf-8-sig"))
        if args.list:
            profile["action"] = "list"
            result = subprocess.run(command(profile), input=json.dumps({k: v for k, v in profile.items() if k not in ("ssh_host", "remote_python")}).encode(), capture_output=True)
            if result.returncode:
                raise ExportError("table discovery failed")
            print(result.stdout.decode("utf-8"))
        else:
            print(json.dumps(export(profile, args.output, args.overwrite, cancel), ensure_ascii=False))
    except Exception as exc:
        print(json.dumps({"status": "failed", "type": type(exc).__name__, "detail": str(exc)}, ensure_ascii=False), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
