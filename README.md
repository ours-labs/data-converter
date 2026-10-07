# PG CSV Exporter

[日本語](README.ja.md)

Export one PostgreSQL table to a local CSV file over SSH. The Windows desktop
application provides connection profiles, a table preview, column selection,
a date range, a destination picker, progress, and cancellation. PostgreSQL
produces CSV using COPY; the client streams it to disk and checks its hash,
header and row count before completing the file.

## Preview status

This is an unsigned, portable Windows x64 preview. CSV export has been exercised
on one Windows PC using an existing SSH configuration. A separate Windows PC,
password authentication, encrypted private keys and Windows Credential Manager
integration have not been validated end to end. There is no installer or automatic
update mechanism. UI language defaults to English and can be switched to Japanese.

## Requirements

- Windows 10/11 x64, Microsoft Edge WebView2 Runtime, and the OpenSSH client.
- An accessible Linux SSH server with Python 3.11+ and psycopg 3 installed.
- PostgreSQL credentials with SELECT access to the chosen table.
- Sufficient free disk space for the CSV. The size estimate samples rows and is approximate.

The app does not install server dependencies or provision SSH keys. New connection
profiles can be entered in the app; existing SSH aliases are also supported.
Profiles are app-specific and do not edit your SSH configuration. Check server
fingerprints against a trusted source before accepting a new host key.

## Run the Windows package

1. Download the Windows x64 preview ZIP and its `.sha256` file from this repository's Releases.
2. Compare `Get-FileHash .\PG-CSV-Exporter-v0.2.0-preview.2-windows-x64.zip -Algorithm SHA256`
   with the checksum file, then extract the entire ZIP into a writable folder.
3. Keep `pgcsv-desktop.exe` and `pgcsv.exe` together and launch `pgcsv-desktop.exe`.
4. Enter the SSH connection, remote Python path and database settings. Check both connections.
5. Select one table, preview it, and choose columns and an optional date range.
6. Select the CSV destination and start export.

Rust, Node.js and Python are not required on the Windows PC to run the package.
If startup is blocked, retain the exact Windows error for troubleshooting; do not
disable protection to run an unverified download.

## CSV and credentials

CSV uses UTF-8 with a BOM and header. The database session timezone is Asia/Tokyo.
A date range includes its start and excludes its end. Timezone-free timestamps
are compared as stored. Row order is unspecified. The 10-row preview is independent
of the export size. Only one table is exported per run. There is no arbitrary SQL
editor, row-limit selector or resumable transfer.

The database transaction is read-only. Cancel before closing during an export;
failed or cancelled exports do not replace the destination with an incomplete CSV.
CSV preserves database text, including values that spreadsheet software may
interpret as formulas. Import untrusted CSV as text rather than enabling formulas.

Profiles omit passwords. Optional credential saving uses Windows Credential Manager.
Protect exported data and connection profiles according to your organization's rules.

## Build and test

Use a current stable Rust toolchain, Node.js 22+, Python 3.11+, and (on Windows)
Microsoft C++ Build Tools. Dependencies are locked in Cargo.lock and package-lock.json.

```powershell
npm ci
npm test
python -m unittest discover -s tests -v
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
cargo test --manifest-path src-tauri/Cargo.toml --locked --features custom-protocol
cargo build --manifest-path src-tauri/Cargo.toml --locked --release --features custom-protocol
```

Desktop builds require Windows. Before distributing a build, use
`scripts/build-public.ps1` to remap build paths, then inspect both executables.
Place the release CLI beside the desktop executable. The CLI also accepts a JSON
profile: `pgcsv.exe PROFILE.json OUTPUT.csv [--overwrite]`. The example profile contains
placeholders and must be adapted to your server.

## License

The application's original code is licensed under [MIT](LICENSE).
Dependencies retain their own licenses. The Windows package includes third-party
notices, upstream source archive links and Rust standard-library notices.

Preview displays text values, truncating each cell after 1024 characters with an ellipsis. It supports up to 256 selected columns and rejects responses above 4 MiB. These limits do not truncate CSV exports.
