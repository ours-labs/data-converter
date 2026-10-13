use crate::{Result, settings};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

static JAPANESE: AtomicBool = AtomicBool::new(false);
/// Render only catalog text; raw errors may contain credentials or server output.
pub fn error_report(detail: &str) -> (String, i32) {
    error_report_in(detail, message("en", "ja"))
}
fn error_report_in(detail: &str, language: &str) -> (String, i32) {
    let catalog: Value = serde_json::from_str(
        include_str!("../ui/error-catalog.mjs")
            .trim()
            .strip_prefix("export default ")
            .unwrap()
            .strip_suffix(';')
            .unwrap(),
    )
    .expect("embedded error catalog must be valid JSON");
    let candidate = detail.split_once(':').map(|(code, _)| code);
    let code = if let Some(code) = candidate.filter(|code| catalog.get(*code).is_some()) {
        code
    } else if detail.eq_ignore_ascii_case("cancelled") || detail == "Input cancelled" {
        "ODC-CANCEL-001"
    } else {
        "ODC-UNKNOWN-001"
    };
    let summary = catalog[code][language]["summary"].as_str().unwrap();
    let label = if language == "ja" {
        "詳細"
    } else {
        "Details"
    };
    (
        format!(
            "Error [{code}]: {summary}\n{label}: https://github.com/ours-labs/data-converter/blob/main/docs/ERRORS.md#{}",
            code.to_lowercase()
        ),
        if code == "ODC-CANCEL-001" { 130 } else { 1 },
    )
}
pub(crate) fn message<'a>(en: &'a str, ja: &'a str) -> &'a str {
    if JAPANESE.load(std::sync::atomic::Ordering::Relaxed) {
        ja
    } else {
        en
    }
}
fn translated(text: &str) -> String {
    translated_in(text, JAPANESE.load(std::sync::atomic::Ordering::Relaxed))
}
fn translated_in(text: &str, japanese: bool) -> String {
    text.lines()
        .map(|line| {
            if let Some((en, ja)) = line.split_once(" / ") {
                if japanese {
                    if en.starts_with('-') {
                        if let Some(end) = en.find("  ") {
                            format!("{}  {ja}", &en[..end])
                        } else {
                            format!("{en}  {ja}")
                        }
                    } else {
                        ja.to_owned()
                    }
                } else {
                    en.to_owned()
                }
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
const HELP: &str = "OURS Data Converter\n\nodc -p [NAME] [-o PATH] [--estimate] [--split-rows N | --no-split]\nodc -d [ssh] TARGET [--non-interactive]\nodc -a [ssh] TARGET [--database NAME]\nodc split INPUT [-o FOLDER] [--split-rows N]\nodc update [--check | --yes | -y]\nodc gui\n\n-h, --help       Help / ヘルプ\n-v, --version    Version / バージョン\n-p, --profile    List profiles, or export NAME / 登録名一覧・指定実行\n-d, --discover   SSH check and PostgreSQL discovery / 接続確認・探索\n-a, --add        Register connection / 接続設定登録\n-o, --output     Destination (default: current directory) / 保存先（既定:現在地）\n--estimate      Read-only estimate / 見積のみ\n--split-rows N  Split by row limit (default 100000) / 分割行数\n--no-split      Disable splitting / 分割なし\n--count-header | --exclude-header-count\n--header | --no-header (local CSV only)\n--encoding utf8|cp932 (local CSV, default utf8)\n--delimiter comma|tab|semicolon (local CSV, default comma)\n--overwrite     Explicit file overwrite only / 明示ファイル上書き\n--non-interactive  Never prompt / 対話なし\n--lang en|ja     Display language / 表示言語\n\nCSV defaults: header present; header counted. Commands do not edit saved export settings.\nCSV既定: ヘッダーあり・行数に含む。実行時の指定は登録設定を変更しません。\nExamples: odc -p sensors --estimate\n          odc -p sensors --split-rows 100000 -o C:\\Exports\n";

#[derive(Debug, Default)]
struct Args {
    command: String,
    positional: Vec<String>,
    options: BTreeMap<String, Option<String>>,
}
fn parse(input: &[String]) -> Result<Args> {
    let mut expanded = Vec::new();
    let mut literal = false;
    for arg in input {
        if !literal && arg == "--" {
            literal = true;
            expanded.push(arg.clone());
        } else if !literal && arg.starts_with("--") && arg.contains('=') {
            let (key, value) = arg.split_once('=').unwrap();
            if ![
                "--profile",
                "--discover",
                "--add",
                "--output",
                "--split-rows",
                "--lang",
                "--database",
                "--encoding",
                "--delimiter",
            ]
            .contains(&key)
                || value.is_empty()
                || value.starts_with('-')
            {
                return Err("Invalid option value".into());
            }
            expanded.extend([key.to_owned(), value.to_owned()]);
        } else {
            expanded.push(arg.clone());
        }
    }
    let args = &expanded;

    let mut a = Args::default();
    let mut i = 0;
    if let Some(first) = args
        .first()
        .filter(|s| matches!(s.as_str(), "split" | "update" | "gui"))
    {
        a.command = first.clone();
        i = 1;
    }
    while i < args.len() {
        if args[i] == "--" {
            a.positional.extend_from_slice(&args[i + 1..]);
            break;
        }
        let key = match args[i].as_str() {
            "-h" => "--help",
            "-v" | "-V" => "--version",
            "-p" => "--profile",
            "-d" => "--discover",
            "-a" => "--add",
            "-o" => "--output",
            "-y" => "--yes",
            s => s,
        }
        .to_owned();
        if !key.starts_with('-') {
            a.positional.push(key);
            i += 1;
            continue;
        }
        let required = matches!(
            key.as_str(),
            "--output" | "--split-rows" | "--lang" | "--database" | "--encoding" | "--delimiter"
        );
        if !required
            && !matches!(
                key.as_str(),
                "--help"
                    | "--version"
                    | "--profile"
                    | "--discover"
                    | "--add"
                    | "--estimate"
                    | "--no-split"
                    | "--count-header"
                    | "--exclude-header-count"
                    | "--header"
                    | "--no-header"
                    | "--overwrite"
                    | "--non-interactive"
                    | "--check"
                    | "--yes"
            )
        {
            return Err(format!("Unknown option: {key}; use odc -h").into());
        }
        let optional = matches!(key.as_str(), "--profile" | "--discover" | "--add");
        let value =
            if (required || optional) && args.get(i + 1).is_some_and(|s| !s.starts_with('-')) {
                i += 1;
                Some(args[i].clone())
            } else {
                None
            };
        if required && value.is_none() {
            return Err(format!("Value required: {key}").into());
        }
        if a.options.insert(key.clone(), value).is_some() {
            return Err(format!("Duplicate option: {key}").into());
        }
        i += 1;
    }
    for (x, y) in [
        ("--split-rows", "--no-split"),
        ("--count-header", "--exclude-header-count"),
        ("--header", "--no-header"),
        ("--check", "--yes"),
    ] {
        if a.has(x) && a.has(y) {
            return Err(format!("Conflicting options: {x} {y}").into());
        }
    }
    if let Some(lang) = a.value("--lang")
        && !["en", "ja"].contains(&lang)
    {
        return Err("Language must be en or ja".into());
    }
    let modes = ["--profile", "--discover", "--add"]
        .iter()
        .filter(|s| a.has(s))
        .count();
    if modes > 1 || (!a.command.is_empty() && modes > 0) {
        return Err("Choose exactly one operation".into());
    }
    let allowed: &[&str] = match a.command.as_str() {
        "update" => &["--check", "--yes", "--non-interactive", "--lang", "--help"],
        "gui" => &["--help"],
        "split" => &[
            "--output",
            "--split-rows",
            "--count-header",
            "--exclude-header-count",
            "--header",
            "--no-header",
            "--encoding",
            "--delimiter",
            "--non-interactive",
            "--lang",
            "--help",
        ],
        _ if a.has("--discover") || a.has("--add") => &[
            "--discover",
            "--add",
            "--database",
            "--non-interactive",
            "--lang",
            "--help",
        ],
        _ => &[
            "--profile",
            "--output",
            "--estimate",
            "--split-rows",
            "--no-split",
            "--count-header",
            "--exclude-header-count",
            "--overwrite",
            "--non-interactive",
            "--lang",
            "--help",
            "--version",
        ],
    };
    if a.options.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err("Option is not valid for this operation; use odc -h".into());
    }
    Ok(a)
}
impl Args {
    fn has(&self, k: &str) -> bool {
        self.options.contains_key(k)
    }
    fn value(&self, k: &str) -> Option<&str> {
        self.options.get(k).and_then(|v| v.as_deref())
    }
    fn interactive(&self) -> bool {
        !self.has("--non-interactive") && io::stdin().is_terminal()
    }
}
fn prompt(label: &str, default: &str, interactive: bool) -> Result<String> {
    let label = translated(label);
    if !interactive {
        return Err(
            format!("Input required: {label}; use the GUI or an interactive terminal").into(),
        );
    }
    eprint!("{label} [{default}]: ");
    io::stderr().flush()?;
    let mut s = String::new();
    if io::stdin().read_line(&mut s)? == 0 {
        return Err("Input cancelled".into());
    }
    let s = s.trim();
    Ok(if s.is_empty() {
        default.into()
    } else {
        s.into()
    })
}
pub(crate) fn confirm(label: &str, interactive: bool) -> Result<bool> {
    Ok(matches!(
        prompt(label, "N", interactive)?.to_lowercase().as_str(),
        "y" | "yes"
    ))
}
fn secret(label: &str, interactive: bool) -> Result<String> {
    if !interactive {
        return Err(format!("Credential required: {label}").into());
    }
    Ok(rpassword::prompt_password(format!("{label}: "))?)
}
fn profiles_path() -> Result<PathBuf> {
    Ok(settings::folder()?.join("profiles.json"))
}
fn safe_name(name: &str) -> String {
    let s: String = name
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| if "<>:\"/\\|?*".contains(c) { '_' } else { c })
        .take(70)
        .collect();
    if s.is_empty() { "data".into() } else { s }
}
fn output_path(a: &Args, table: &str, split: bool) -> Result<PathBuf> {
    let path = a
        .value("--output")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    if split {
        if !path.is_dir() {
            return Err("Split destination must be an existing folder".into());
        }
        return Ok(path);
    }
    if a.value("--output").is_some() && !path.is_dir() {
        return Ok(path);
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    for n in 0..10000 {
        let file = path.join(format!("{}_{stamp}_{n:03}.csv", safe_name(table)));
        if !file.exists() {
            return Ok(file);
        }
    }
    Err("Unable to choose output filename".into())
}
fn split_options(a: &Args, saved: Option<&Value>, local: bool) -> Result<Value> {
    let mut v = saved.cloned().unwrap_or(
        json!({"limit":100000,"header":true,"count_header":true,"delimiter":",","encoding":"utf8"}),
    );
    if let Some(n) = a.value("--split-rows") {
        v["limit"] = json!(n.parse::<u64>().map_err(|_| "Invalid row limit")?)
    }
    if a.has("--count-header") {
        v["count_header"] = json!(true)
    }
    if a.has("--exclude-header-count") {
        v["count_header"] = json!(false)
    }
    if local {
        if a.has("--no-header") {
            v["header"] = json!(false)
        }
        if a.has("--header") {
            v["header"] = json!(true)
        }
        if let Some(e) = a.value("--encoding") {
            v["encoding"] = json!(e)
        }
        if let Some(d) = a.value("--delimiter") {
            v["delimiter"] = json!(match d {
                "comma" => ",",
                "tab" => "\t",
                "semicolon" => ";",
                _ => return Err("Invalid delimiter".into()),
            })
        }
    }
    crate::split::Options::from_value(&v)?;
    Ok(v)
}
fn prepare(p: &mut Value, interactive: bool) -> Result<tempfile::NamedTempFile> {
    let host = tempfile::NamedTempFile::new()?;
    p["_askpass"] = json!(std::env::current_exe()?);
    p["_host_prompt_path"] = json!(host.path());
    if p["auth_mode"] == "password" && p["_ssh_secret"].as_str().unwrap_or("").is_empty() {
        p["_ssh_secret"] = json!(secret("SSH password", interactive)?)
    }
    Ok(host)
}
fn query(p: &mut Value, action: &str, interactive: bool) -> Result<Value> {
    let host = prepare(p, interactive)?;
    p["action"] = json!(action);
    let answer = crate::query(p);
    let question = fs::read_to_string(host.path())?;
    if !question.is_empty() && p["_host_expected"].as_str() != Some(&question) {
        eprintln!("{question}");
        if !confirm(
            "Trust this SSH host key? / ホスト鍵を信頼しますか",
            interactive,
        )? {
            return Err("Host key not approved".into());
        }
        p["_host_expected"] = json!(question);
        return crate::query(p);
    }
    answer
}
fn db_credentials(p: &mut Value, interactive: bool) -> Result<()> {
    if p["_database"].is_object() && p["_database"]["password"].as_str().unwrap_or("").is_empty() {
        p["_database"]["password"] = json!(secret("PostgreSQL password", interactive)?)
    }
    Ok(())
}
fn target(a: &Args, key: &str) -> Result<String> {
    let value = a.value(key).ok_or("SSH target required")?;
    let t = if value == "ssh" {
        if a.positional.len() != 1 {
            return Err("Expected ssh USER@HOST".into());
        }
        &a.positional[0]
    } else {
        if !a.positional.is_empty() {
            return Err("Unexpected argument".into());
        }
        value
    };
    crate::ssh_base(&json!({"ssh_host":t}))?;
    Ok(t.into())
}
fn connection(a: &Args, add: bool) -> Result<()> {
    let host = target(a, if add { "--add" } else { "--discover" })?;
    let all = settings::read(&profiles_path()?)?;
    let matching = all
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, p)| p["ssh_host"].as_str() == Some(&host));
    let mut p = if let Some((name, _)) = matching {
        settings::load(&profiles_path()?, name)?
    } else {
        json!({"ssh_host":host})
    };
    if matching.is_none() && a.interactive() {
        let auth = prompt(
            "SSH authentication: existing, password, key",
            "existing",
            true,
        )?;
        if !["existing", "password", "key"].contains(&auth.as_str()) {
            return Err("Invalid SSH authentication mode".into());
        }
        if auth == "key" {
            p["identity_file"] = json!(prompt("Private key path", "", true)?);
            p["_ssh_secret"] = json!(secret("Key passphrase (empty if none)", true)?);
        }
        p["auth_mode"] = json!(auth);
    }
    query(&mut p, "connect", a.interactive())?;
    println!("{}: {host}", message("SSH connected", "SSH接続成功"));
    let prompt_file = prepare(&mut p, a.interactive())?;
    let mut cmd = crate::ssh_base(&p)?;
    cmd.arg(&host).arg("cat >/dev/null; if command -v ss >/dev/null 2>&1; then printf '{\"ports\":['; ss -H -lnt 2>/dev/null | awk '{n=split($4,a,\":\"); if(a[n] ~ /^[0-9]+$/) print a[n]}' | sort -nu | head -64 | awk 'BEGIN{s=\"\"}{printf \"%s%s\",s,$1;s=\",\"}'; printf '],\"scope\":\"visible TCP listeners; at most 64 ports; type and inaccessible services unverified\"}'; else printf '{\"ports\":[],\"scope\":\"ss unavailable; listeners unverified\"}'; fi");
    match crate::query_command(&json!({}), cmd) {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
        Err(_) => println!("{}", message("Discovery incomplete", "探索未完了")),
    };
    for (label, probe) in [
        (
            "PostgreSQL service",
            "cat >/dev/null; if command -v systemctl >/dev/null 2>&1; then if systemctl list-unit-files --no-pager --no-legend 'postgresql*' 2>/dev/null | grep -q postgresql; then printf '{\"state\":\"candidate found\"}'; else printf '{\"state\":\"unverified\"}'; fi; else printf '{\"state\":\"unavailable\"}'; fi",
        ),
        (
            "PostgreSQL container",
            "cat >/dev/null; if command -v docker >/dev/null 2>&1; then if docker ps --format '{{.Image}}' 2>/dev/null | grep -qi postgres; then printf '{\"state\":\"candidate found\"}'; else printf '{\"state\":\"unverified\"}'; fi; else printf '{\"state\":\"unavailable\"}'; fi",
        ),
    ] {
        let mut command = crate::ssh_base(&p)?;
        command.arg(&host).arg(probe);
        match crate::query_command(&json!({}), command) {
            Ok(value) => println!("{label}: {value}"),
            Err(_) => println!("{label}: unverified"),
        }
    }
    drop(prompt_file);
    if add || p.get("_database").is_none() && p.get("env_file").is_none() {
        if !a.interactive() {
            println!(
                "{}",
                message(
                    "DB names unverified: credentials required",
                    "DB名は未確認です。認証情報が必要です"
                )
            );
            return if add {
                Err("Registration requires interactive input".into())
            } else {
                Ok(())
            };
        }
        if !add && !confirm("Check database names? / DB名を確認しますか", true)? {
            return Ok(());
        }
        let initial = match a.value("--database") {
            Some(db) => db.to_owned(),
            None => prompt("Initial database", "postgres", true)?,
        };
        p["_database"] = json!({"host":prompt("DB host","127.0.0.1",true)?,"port":prompt("DB port","5432",true)?.parse::<u16>()?,"dbname":initial,"user":prompt("DB user","",true)?});
        p.as_object_mut().unwrap().remove("env_file");
    }
    if let Some(db) = a.value("--database") {
        p["database_name"] = json!(db)
    }
    db_credentials(&mut p, a.interactive())?;
    let names = query(&mut p, "databases", a.interactive())?;
    println!("{}", serde_json::to_string_pretty(&names)?);
    if add {
        let db = prompt(
            "Database",
            p["database_name"]
                .as_str()
                .or(names["current"].as_str())
                .unwrap_or(""),
            a.interactive(),
        )?;
        if !names["names"]
            .as_array()
            .is_some_and(|ns| ns.iter().any(|n| n.as_str() == Some(&db)))
        {
            return Err("Choose a listed database".into());
        }
        p["database_name"] = json!(db);
        let name = prompt("Profile name / 登録名", &host, a.interactive())?;
        println!("{name}: {host} / {}", p["database_name"]);
        if !confirm("Save connection? / 保存しますか", a.interactive())? {
            return Ok(());
        }
        let replace = all.get(&name).is_some();
        if replace
            && !confirm(
                "Replace existing profile? / 同名設定を置換しますか",
                a.interactive(),
            )?
        {
            return Ok(());
        }
        let save = confirm(
            "Save passwords in credential store? / 資格情報を保存しますか",
            a.interactive(),
        )?;
        for key in [
            "schema",
            "table",
            "columns",
            "time_column",
            "start",
            "end",
            "output_path",
            "split_options",
        ] {
            p.as_object_mut().unwrap().remove(key);
        }
        settings::save(&profiles_path()?, &name, &p, save, replace)?;
        println!(
            "{}",
            message(
                "Saved. Configure table and columns in GUI",
                "保存しました。テーブル・列はGUIで設定してください。"
            )
        );
    }
    Ok(())
}
fn register_path(args: &[String]) -> Result<()> {
    if !cfg!(windows) || args.len() != 1 || !["add", "remove"].contains(&args[0].as_str()) {
        return Err("Invalid PATH operation".into());
    }
    let exe = std::env::current_exe()?;
    let dir = exe.parent().ok_or("Missing install directory")?;
    if dir.to_string_lossy().contains(';') {
        return Err("Install path contains semicolon".into());
    }
    let script = r#"$ErrorActionPreference='Stop'
$d=$env:ODC_INSTALL_DIR
$m=Join-Path $d '.path-added'
$p=[Environment]::GetEnvironmentVariable('Path','User')
$items=@($p -split ';' | Where-Object { $_ -ne '' })
if($env:ODC_PATH_ACTION -eq 'add') {
  if(-not ($items | Where-Object { $_.TrimEnd('\') -ieq $d.TrimEnd('\') })) {
    [IO.File]::WriteAllText($m,'owned')
    [Environment]::SetEnvironmentVariable('Path',(($items + $d) -join ';'),'User')
  }
} elseif(Test-Path -LiteralPath $m) {
  $items=@($items | Where-Object { $_.TrimEnd('\') -ine $d.TrimEnd('\') })
  [Environment]::SetEnvironmentVariable('Path',($items -join ';'),'User')
  Remove-Item -LiteralPath $m
}
"#;
    let shell = PathBuf::from(std::env::var_os("SystemRoot").ok_or("SystemRoot unavailable")?)
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let mut cmd = std::process::Command::new(shell);
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("ODC_INSTALL_DIR", dir)
        .env("ODC_PATH_ACTION", &args[0]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    if !cmd.status()?.success() {
        return Err("User PATH registration failed".into());
    }
    Ok(())
}
pub fn run() -> Result<()> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().is_some_and(|s| s == "__install_verified") {
        return crate::update::install(&raw[1..]);
    }
    if raw.first().is_some_and(|s| s == "__path") {
        return register_path(&raw[1..]);
    }
    // Set language before parsing so invalid arguments also respect the preference.
    let mut language = settings::language();
    let mut args = raw.iter().take_while(|s| s.as_str() != "--");
    while let Some(arg) = args.next() {
        let value = if arg == "--lang" {
            args.next().map(String::as_str)
        } else {
            arg.strip_prefix("--lang=")
        };
        if let Some(value @ ("ja" | "en")) = value {
            language = value.to_owned();
        }
    }
    JAPANESE.store(language == "ja", std::sync::atomic::Ordering::Relaxed);
    let a = parse(&raw).map_err(|error| format!("ODC-CONFIG-001: {error}"))?;
    let lang = a
        .value("--lang")
        .map(str::to_owned)
        .unwrap_or_else(settings::language);
    JAPANESE.store(lang == "ja", std::sync::atomic::Ordering::Relaxed);
    if raw.is_empty() {
        println!(
            "OURS Data Converter\nodc -h  Help\nodc -p  Profiles\nodc -d TARGET  Discover\nodc -a TARGET  Register\nodc split INPUT\nodc update [--check | -y]\nodc gui"
        );
        return Ok(());
    }
    if a.has("--help") {
        let help = translated(HELP);
        println!(
            "{}",
            help.lines()
                .filter(|line| JAPANESE.load(std::sync::atomic::Ordering::Relaxed)
                    || !line.starts_with("CSV既定"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        return Ok(());
    }
    if a.has("--version") {
        println!("OURS Data Converter {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if a.command == "update" {
        if !a.positional.is_empty() {
            return Err("Unexpected argument".into());
        }
        return crate::update::run(a.has("--check"), a.has("--yes"), a.interactive());
    }
    let _runtime = settings::runtime_lock(false)?;
    if a.command == "gui" {
        if !a.positional.is_empty() {
            return Err("Unexpected argument".into());
        }
        std::process::Command::new(std::env::current_exe()?.with_file_name("pgcsv-desktop.exe"))
            .spawn()?;
        return Ok(());
    }
    if a.has("--discover") || a.has("--add") {
        return connection(&a, a.has("--add"));
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    ctrlc::set_handler(move || flag.store(true, std::sync::atomic::Ordering::Relaxed))?;
    if a.command == "split" {
        if a.positional.len() != 1 {
            return Err("Usage: odc split INPUT [-o FOLDER] [--split-rows N]".into());
        }
        let input = Path::new(&a.positional[0]);
        let options = crate::split::Options::from_value(&split_options(&a, None, true)?)?;
        let output = output_path(&a, "data", true)?;
        println!("{}: {}", message("Destination", "保存先"), output.display());
        println!(
            "{}",
            crate::split::split_file(
                input,
                &output,
                options,
                &cancel,
                &std::sync::atomic::AtomicU64::new(0)
            )?
        );
        return Ok(());
    }
    if !a.positional.is_empty() || !a.has("--profile") {
        return Err("Specify -p NAME; use odc -h".into());
    }
    let Some(name) = a.value("--profile") else {
        if a.options
            .keys()
            .any(|s| !matches!(s.as_str(), "--profile" | "--lang" | "--non-interactive"))
        {
            return Err("Profile name required".into());
        }
        let all = settings::read(&profiles_path()?)?;
        if all.as_object().unwrap().is_empty() {
            println!(
                "{}",
                message("No saved profiles", "登録済み設定はありません")
            )
        }
        for name in all.as_object().unwrap().keys() {
            println!("{name}")
        }
        return Ok(());
    };
    let mut p = settings::load(&profiles_path()?, name)?;
    if p["table"].as_str().is_none_or(str::is_empty) {
        return Err(
            "Select a table and columns in GUI / GUIでテーブルと列を設定してください".into(),
        );
    }
    let split = !a.has("--no-split") && (a.has("--split-rows") || p.get("split_options").is_some());
    if split {
        p["_split"] = split_options(&a, p.get("split_options"), false)?;
    } else if a.has("--count-header") || a.has("--exclude-header-count") {
        return Err("Header counting requires splitting".into());
    }
    let output = output_path(&a, p["table"].as_str().unwrap(), split)?;
    println!(
        "{}: {} / {}\n{}: {}",
        message("Source", "対象"),
        name,
        p["table"],
        message("Destination", "保存先"),
        output.display()
    );
    db_credentials(&mut p, a.interactive())?;
    query(&mut p, "connect", a.interactive())?;
    if a.has("--estimate") {
        let v = query(&mut p, "estimate", a.interactive())?;
        println!("{}", serde_json::to_string_pretty(&v)?);
        if split {
            let limit = p["_split"]["limit"].as_u64().ok_or("Invalid limit")?
                - u64::from(p["_split"]["count_header"] == true);
            println!(
                "{}: {}",
                message("Files", "ファイル数"),
                v["rows"].as_u64().ok_or("Invalid count")?.div_ceil(limit)
            );
        }
        return Ok(());
    }
    let _host = prepare(&mut p, a.interactive())?;
    println!(
        "{}",
        crate::export(
            &p,
            &output,
            a.has("--overwrite") && a.value("--output").is_some_and(|s| !Path::new(s).is_dir()),
            cancel
        )?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(s: &[&str]) -> Result<Args> {
        parse(&s.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }
    #[test]
    fn command_contract() {
        let help = translated_in(HELP, true);
        assert!(help.contains("-h, --help  ヘルプ"));
        assert!(help.contains("--split-rows N  分割行数"));
        assert!(translated_in(HELP, false).contains("-h, --help       Help"));
        assert!(args(&["-p"]).unwrap().value("--profile").is_none());
        assert_eq!(
            args(&["-p", "日本語"]).unwrap().value("--profile"),
            Some("日本語")
        );
        for a in [&["update", "-y"][..], &["update", "--yes"]] {
            assert!(args(a).unwrap().has("--yes"));
        }
        for a in [
            &["update", "--check", "-y"][..],
            &["-p", "x", "--no-split", "--split-rows", "3"],
            &["-p", "a", "-p", "b"],
            &["-o"],
            &["--bogus"],
            &["split", "a", "--overwrite"],
            &["-p", "a", "--database", "x"],
        ] {
            assert!(args(a).is_err(), "{a:?}")
        }
    }
    #[test]
    fn destination_defaults() {
        assert_eq!(
            args(&["--profile=name"]).unwrap().value("--profile"),
            Some("name")
        );
        assert_eq!(
            args(&["split", "--", "-input.csv"]).unwrap().positional,
            vec!["-input.csv"]
        );
        assert!(args(&["update", "--yes=false"]).is_err());
        let a = args(&["-p", "x"]).unwrap();
        assert_eq!(
            output_path(&a, "a/b", false).unwrap().parent().unwrap(),
            std::env::current_dir().unwrap()
        );
        let a = args(&["-p", "x", "-o", "explicit.csv"]).unwrap();
        assert_eq!(
            output_path(&a, "t", false).unwrap(),
            PathBuf::from("explicit.csv")
        );
        assert!(output_path(&a, "t", true).is_err());
    }
    #[test]
    fn split_defaults() {
        let a = args(&["split", "file"]).unwrap();
        let o = split_options(&a, None, true).unwrap();
        assert_eq!(o["limit"], 100000);
        assert_eq!(o["header"], true);
        assert_eq!(o["count_header"], true);
        assert!(
            split_options(
                &args(&["split", "file", "--split-rows", "1"]).unwrap(),
                None,
                true
            )
            .is_err()
        );
    }
    #[test]
    fn targets_are_not_shell() {
        assert_eq!(
            target(
                &args(&["-d", "ssh", "pi@host.local"]).unwrap(),
                "--discover"
            )
            .unwrap(),
            "pi@host.local"
        );
        assert!(target(&args(&["-d", "host;id"]).unwrap(), "--discover").is_err());
    }
}
