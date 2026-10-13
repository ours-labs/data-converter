"""検証済みexeと公開範囲を限定した説明・noticeをZIPへまとめる。"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "docs/distribution"
CONTENT_NAMES = {"pgcsv-desktop.exe", "pgcsv.exe", "README.txt", "README.ja.txt",
                 "THIRD-PARTY-NOTICES.txt", "RUST-STANDARD-LIBRARY-NOTICES.html"}


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def validate_exe(data: bytes) -> None:
    if len(data) < 64 or data[:2] != b"MZ":
        raise ValueError("Windows exeではありません。")
    offset = int.from_bytes(data[60:64], "little")
    if data[offset:offset+6] != b"PE\0\0\x64\x86":
        raise ValueError("Windows x64 exeではありません。")


def notices(metadata: dict, assets: Path = ASSETS) -> bytes:
    sources = json.loads((assets / "licenses/sources.json").read_text(encoding="utf-8"))
    lines = ["Third-party notices\n\nIncludes the locked Cargo dependency set, including build-time dependencies.\n"
             "Upstream package sources are unmodified. Exact source archives are linked below.\n"]
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if not package.get("source"):
            continue
        directory = Path(package["manifest_path"]).parent
        texts = [(f.name, f.read_bytes()) for f in sorted(directory.iterdir()) if f.is_file()
                 and f.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE"))]
        if not texts:
            for source in sources:
                if source["package"] == package["name"]:
                    data = (assets / "licenses" / source["path"]).read_bytes()
                    if sha(data) != source["sha256"]:
                        raise ValueError("license原文hashが不一致です。")
                    texts.append((source["url"], data))
        if not texts:
            raise ValueError(f"license原文未確認: {package['name']} {package['version']}")
        name, version = package["name"], package["version"]
        lines.append(f"\n=== {name} {version} ===\nLicense: {package.get('license')}\n"
                     f"Source: https://crates.io/api/v1/crates/{name}/{version}/download\n")
        for label, data in texts:
            lines.append(f"\n--- {label} ---\n" + data.decode("utf-8") + "\n")
    return "".join(lines).encode("utf-8")


def write_package(folder: Path, destination: Path, version: str, binding: dict,
                  notice: bytes, rust_notice: bytes, assets: Path = ASSETS) -> dict:
    if not re.fullmatch(r"\d+\.\d+\.\d+-preview\.\d+", version):
        raise ValueError("試用版versionはx.y.z-preview.nにしてください。")
    content = {}
    for name, expected in binding["sha256"].items():
        if name not in {"pgcsv-desktop.exe", "pgcsv.exe"}:
            raise ValueError("未許可exeです。")
        data = (folder / name).read_bytes()
        validate_exe(data)
        if sha(data) != expected:
            raise ValueError(f"検証済みexeとhashが異なります: {name}")
        content[name] = data
    if set(content) != {"pgcsv-desktop.exe", "pgcsv.exe"}:
        raise ValueError("2つのexeが必要です。")
    for name in ["README.txt", "README.ja.txt"]:
        # UTF-8 BOM preserves Japanese in common Windows text viewers.
        content[name] = (assets / name).read_text(encoding="utf-8").encode("utf-8-sig")
    content["THIRD-PARTY-NOTICES.txt"] = notice
    content["RUST-STANDARD-LIBRARY-NOTICES.html"] = rust_notice
    if set(content) != CONTENT_NAMES:
        raise ValueError("配布allowlistが不一致です。")
    manifest = {"version": version, "binary_source_commit": binding["binary_source_commit"],
                "application_version": binding["application_version"], "platform": "windows-x64",
                "other_windows_pc_tested": False, "files": {n: sha(d) for n, d in content.items()}}
    content["MANIFEST.json"] = (json.dumps(manifest, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
    destination.parent.mkdir(parents=True, exist_ok=True)
    # Do not overwrite a previously distributed artifact silently.
    with zipfile.ZipFile(destination, "x", compression=zipfile.ZIP_DEFLATED) as archive:
        for name, data in sorted(content.items()):
            archive.writestr("PG CSV Exporter/" + name, data)
    verify_package(destination)
    checksum = sha(destination.read_bytes())
    destination.with_suffix(".zip.sha256").write_text(f"{checksum}  {destination.name}\n", encoding="ascii")
    return {"zip": str(destination), "sha256": checksum, "bytes": destination.stat().st_size,
            "binary_source_commit": binding["binary_source_commit"]}


def verify_package(path: Path) -> dict:
    with zipfile.ZipFile(path) as archive:
        expected = {"PG CSV Exporter/" + n for n in CONTENT_NAMES | {"MANIFEST.json"}}
        if set(archive.namelist()) != expected or len(archive.namelist()) != len(expected):
            raise ValueError("ZIPの内容がallowlistと一致しません。")
        manifest = json.loads(archive.read("PG CSV Exporter/MANIFEST.json"))
        if set(manifest["files"]) != CONTENT_NAMES:
            raise ValueError("manifestのfile集合が不一致です。")
        for name, expected_hash in manifest["files"].items():
            if sha(archive.read("PG CSV Exporter/" + name)) != expected_hash:
                raise ValueError("ZIP内file hashが不一致です。")
        for name in ["pgcsv-desktop.exe", "pgcsv.exe"]:
            validate_exe(archive.read("PG CSV Exporter/" + name))
        archive.read("PG CSV Exporter/README.ja.txt").decode("utf-8-sig")
        return manifest


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--rust-notices", type=Path, required=True)
    parser.add_argument("--version", default="0.2.0-preview.1")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binding = json.loads((ASSETS / "binaries.json").read_text(encoding="utf-8"))
    changed = subprocess.check_output(["git", "-C", str(ROOT), "diff", binding["binary_source_commit"],
                                      "--name-only", "--", "src", "pgcsv", "ui", "src-tauri",
                                      "Cargo.toml", "Cargo.lock", ".cargo"])
    if changed.strip():
        raise ValueError("runtime sourceが変更されています。新しいexe検証・bindingが必要です。")
    metadata = json.loads(args.metadata.read_text(encoding="utf-8-sig"))
    print(json.dumps(write_package(ROOT / "dist/PG CSV Exporter", args.output, args.version, binding,
                                   notices(metadata), args.rust_notices.read_bytes()), ensure_ascii=False))


if __name__ == "__main__":
    main()
