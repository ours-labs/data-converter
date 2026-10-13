# OURS Data Converter

[日本語](README.ja.md)

Export one PostgreSQL table to a local CSV file over SSH. The Windows desktop
application provides connection profiles, a table preview, column selection,
a date range, a destination picker, progress, and cancellation. PostgreSQL
produces CSV using COPY; the client streams it to disk, calculates a receive hash,
and checks the header and row count before completing the file.

## Preview status

Version 0.4.0 provides an unsigned Windows x64 installer and portable package.
Exports and command-line registration with database credentials have been exercised
on one Windows PC using an existing SSH configuration. A separate Windows PC,
SSH password authentication and encrypted private keys remain unverified.
UI language defaults to English and can be switched to Japanese.
Updates are user-initiated; no background automatic updates run.

## Requirements

- Windows 10/11 x64, Microsoft Edge WebView2 Runtime, and the OpenSSH client.
- An accessible Linux SSH server with SSH forwarding enabled and TCP access to PostgreSQL.
- PostgreSQL credentials with SELECT access to the chosen table.
- Sufficient free disk space for the CSV. The size estimate samples rows and is approximate.

The app does not install server dependencies or provision SSH keys. New connection
profiles can be entered in the app; existing SSH aliases are also supported.
Profiles are app-specific and do not edit your SSH configuration. Check server
fingerprints against a trusted source before accepting a new host key.

## Run the Windows package

1. Download the Windows x64 installer or ZIP and its `.sha256` file from Releases.
2. Verify the download with `Get-FileHash <downloaded-file> -Algorithm SHA256`.
3. Run the installer for the current user and open a new terminal to use `odc`.
   For the ZIP, extract all files together and launch `pgcsv-desktop.exe`;
   run `odc.exe` from that folder. Portable extraction does not register PATH.
4. Enter the SSH connection, database settings. Check both connections.
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

## Split CSV files

Choose **Split an existing CSV** to split a local file without SSH. Select the input,
parent folder, encoding and delimiter. Alternatively enable **Split while exporting**
on the PostgreSQL destination screen. A new folder contains numbered CSV files.
Headers are repeated in every file. The default limit is 100,000 records including
the header (99,999 data records); counting the header is optional. Local files can
also be declared headerless. Newlines inside quoted fields do not count as records.

Local splitting preserves the source bytes, quoting, order, delimiter and BOM.
UTF-8 is validated by default; choose CP932 explicitly when needed. Comma, tab and
semicolon are supported. Malformed quoting, inconsistent field counts, invalid
encoding or a record larger than 64 MiB stops the operation without skipping data.
Empty/header-only input creates no output. Cancelled or failed output retains
INCOMPLETE.txt; this marker is removed only after successful validation and syncing.
The original local CSV is never changed. There is no resumable split operation.

## Connection transport and errors

The Rust driver connects through OpenSSH standard I/O forwarding. Legacy remote_python settings are ignored. Only one TCP endpoint is supported; Unix sockets, multiple hosts and hostaddr are unsupported. Database connections beyond the SSH server loopback require TLS with certificate and hostname verification using the OS trust store. Server settings files are read with head into PC memory (64 KiB maximum); environment variables use printenv. Setting values are never executed.

See the [error reference](docs/ERRORS.md). Bundled advice and expandable details work offline. If online documentation is unavailable, use the bundled reference. Stream hashes are calculated on the PC. Very large individual rows can also require large driver receive buffers.

The source tree also retains a separate Python comparison CLI for development.
That legacy entry point still requires remote Python and psycopg; it is not used
by the Windows package or the Rust CLI. Linux CLI builds need OpenSSL development
headers and pkg-config. The native error layout has automated DOM coverage;
visual verification of this preview in the Windows desktop remains pending.

## Command line

Run `odc` for a short command list and `odc -h` for full help.

```text
odc -p
odc -d user@example-host
odc -a user@example-host
odc -p my-profile --estimate
odc -p my-profile --split-rows 100000 -o C:\Exports
odc split input.csv --split-rows 100000
odc update --check
odc update
odc update -y
```

`-p` lists saved profiles one per line. `-d` checks SSH and discovers accessible
PostgreSQL candidates; incomplete discovery does not prove absence. `-a` registers
an existing connection interactively; select the table and columns in the GUI.
Omitting `-o` saves to the current directory, even if the GUI profile has a destination.
Headers are repeated and count toward the row limit by default; use
`--exclude-header-count` to count data records only. Splits create a new subfolder.

`update --yes` and `update -y` are equivalent. They skip the application's confirmation,
not integrity checks or OS prompts. Close other instances before updating.
Only published stable Windows installers are accepted; missing release information
is an error, not confirmation that the installed version is current.
Public-release update completion has not yet been validated end to end.

Errors print a code, summary and a link to [the error reference](docs/ERRORS.md).
The browser does not open automatically. Raw errors and credentials are not printed.
