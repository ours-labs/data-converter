import hashlib
import json
from pathlib import Path
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch
from pgcsv.exporter import command, export, ExportError, validate_config


class ExportTests(unittest.TestCase):
    profile = {"ssh_host": "example", "remote_python": "/usr/bin/python3", "table": "readings"}

    def worker(self, data, rows=2, digest=None, fail=False, delay=0):
        start = {"event":"start", "columns":["name", "value"], "snapshot":"fixture"}
        complete = {"event":"complete", "rows":rows, "bytes":len(data), "sha256":digest or hashlib.sha256(data).hexdigest(), "stream_seconds":0.01}
        code = "import sys,time,json;sys.stdin.read();"
        code += "time.sleep(%r);" % delay
        code += "sys.stderr.write(%r);" % ("PGCSV " + json.dumps(start) + "\n")
        code += "sys.stdout.buffer.write(%r);sys.stdout.buffer.flush();" % data
        code += "sys.stderr.write(%r);sys.exit(%d)" % ("PGCSV " + json.dumps(complete) + "\n", int(fail))
        return [sys.executable, "-c", code]

    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)
        self.output = Path(self.folder.name) / "日本語.csv"
        self.data = 'name,value\n"日本語,引用""\n改行",\n"",1\n'.encode()

    def test_byte_preserving_japanese_and_embedded_newline(self):
        with patch("pgcsv.exporter.command", return_value=self.worker(self.data)):
            result = export(self.profile, self.output)
        self.assertEqual(self.output.read_bytes(), b"\xef\xbb\xbf" + self.data)
        self.assertEqual(result["rows"], 2)

    def test_overwrite_rejected_and_existing_preserved_on_remote_failure(self):
        self.output.write_bytes(b"previous")
        with self.assertRaises(FileExistsError):
            export(self.profile, self.output)
        with patch("pgcsv.exporter.command", return_value=self.worker(self.data, fail=True)):
            with self.assertRaises(ExportError):
                export(self.profile, self.output, overwrite=True)
        self.assertEqual(self.output.read_bytes(), b"previous")
        self.assertEqual(list(self.output.parent.glob("*.part")), [])

    def test_digest_and_count_mismatch_do_not_publish(self):
        for arguments in ({"digest":"bad"}, {"rows":3}):
            with self.subTest(arguments=arguments), patch("pgcsv.exporter.command", return_value=self.worker(self.data, **arguments)):
                with self.assertRaises(ExportError):
                    export(self.profile, self.output)
                self.assertFalse(self.output.exists())

    def test_empty_table(self):
        with patch("pgcsv.exporter.command", return_value=self.worker(b"name,value\n", rows=0)):
            self.assertEqual(export(self.profile, self.output)["rows"], 0)

    def test_cancel(self):
        cancelled = threading.Event()
        timer = threading.Timer(0.2, cancelled.set)
        with patch("pgcsv.exporter.command", return_value=self.worker(self.data, delay=5)):
            timer.start()
            with self.assertRaises(ExportError):
                export(self.profile, self.output, cancel=cancelled)
        timer.join()
        self.assertFalse(self.output.exists())

    def test_invalid_column_count(self):
        with patch("pgcsv.exporter.command", return_value=self.worker(b"name,value\na\n", rows=1)):
            with self.assertRaises(ExportError):
                export(self.profile, self.output)

    def test_save_failure_preserves_existing(self):
        self.output.write_bytes(b"previous")
        with patch("pgcsv.exporter.command", return_value=self.worker(self.data)), patch("pgcsv.exporter.os.fsync", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                export(self.profile, self.output, overwrite=True)
        self.assertEqual(self.output.read_bytes(), b"previous")

    def test_host_injection(self):
        for host in ("-option", "host;touch /tmp/bad", "host\n"):
            with self.assertRaises(ValueError):
                command(dict(self.profile, ssh_host=host))

    def test_config_validation(self):
        for extra in ({"password":"secret"}, {"columns":[]}, {"columns":["temp","temp"]}, {"start":"2026-10-02", "end":"2026-10-01", "time_column":"measured_at"}):
            with self.assertRaises(ValueError):
                validate_config(dict(self.profile, **extra))


if __name__ == "__main__":
    unittest.main()
