"""固定Cargo依存とRust標準ライブラリのnoticeをinstaller resourceへ生成する。"""
import importlib.util
import json
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('package_release', ROOT / 'scripts/package-release.py')
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)
metadata = json.loads(subprocess.check_output(['cargo','metadata','--manifest-path',str(ROOT/'src-tauri/Cargo.toml'),'--locked','--offline','--filter-platform','x86_64-pc-windows-msvc','--format-version','1']))
output=ROOT/'local/installer-notices'
output.mkdir(parents=True,exist_ok=True)
(output/'THIRD-PARTY-NOTICES.txt').write_bytes(package.notices(metadata))
sysroot=Path(subprocess.check_output(['rustc','--print','sysroot'],text=True).strip())
rust_notice=sysroot/'share/doc/rust/COPYRIGHT-library.html'
if not rust_notice.is_file():
    raise RuntimeError('Rust standard library notice missing; install rust-docs')
shutil.copyfile(rust_notice,output/'RUST-STANDARD-LIBRARY-NOTICES.html')
