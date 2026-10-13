#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod history;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Default)]
struct Session {
    cancel: Mutex<Option<Arc<AtomicBool>>>,
    result: Mutex<Option<Value>>,
    received: Arc<AtomicU64>,
    scan: Mutex<Scan>,
    scan_gate: Arc<Mutex<()>>,
}

#[derive(Default)]
struct Scan {
    last_id: u64,
    active: Option<Arc<AtomicBool>>,
}
impl Scan {
    fn begin(&mut self, id: u64) -> Result<Arc<AtomicBool>, String> {
        if id <= self.last_id {
            return Err("Cancelled".into());
        }
        self.cancel(id);
        let flag = Arc::new(AtomicBool::new(false));
        self.active = Some(flag.clone());
        Ok(flag)
    }
    fn cancel(&mut self, id: u64) {
        if id >= self.last_id {
            self.last_id = id;
            if let Some(flag) = self.active.take() {
                flag.store(true, Ordering::Relaxed);
            }
        }
    }
}

fn prepare(mut profile: Value, prompt: &std::path::Path) -> Result<Value, String> {
    let helper = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name("pgcsv.exe");
    if !helper.is_file() {
        return Err("同じフォルダーにpgcsv.exeが必要です。".into());
    }
    profile["_askpass"] = json!(helper);
    profile["_host_prompt_path"] = json!(prompt);
    Ok(profile)
}

#[tauri::command]
fn open_error_help(code: String) -> Result<(), String> {
    let data = include_str!("../../ui/error-catalog.mjs")
        .trim()
        .strip_prefix("export default ")
        .and_then(|s| s.strip_suffix(';'))
        .ok_or("Invalid error catalog")?;
    let catalog: Value = serde_json::from_str(data).map_err(|_| "Invalid error catalog")?;
    if catalog.get(&code).is_none() {
        return Err("Unknown error code".into());
    }
    let url = format!(
        "https://github.com/ours-labs/data-converter/blob/main/docs/ERRORS.md#{}",
        code.to_lowercase()
    );
    std::process::Command::new("explorer.exe")
        .arg(url)
        .spawn()
        .map_err(|_| "Unable to open browser")?;
    Ok(())
}

#[tauri::command]
async fn inspect(app: tauri::AppHandle, profile: Value, action: String) -> Result<Value, String> {
    if !["connect", "ssh", "databases", "list", "preview", "estimate"].contains(&action.as_str()) {
        return Err("invalid action".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let temp = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
        let mut prepared = prepare(profile, temp.path())?;
        prepared["action"] = json!(action);
        let history_path = app
            .path()
            .app_config_dir()
            .ok()
            .map(|p| p.join("connection-history.json"));
        inspect_prepared(&prepared, temp.path(), history_path.as_deref(), &action)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn inspect_prepared(
    prepared: &Value,
    prompt_path: &std::path::Path,
    history_path: Option<&std::path::Path>,
    action: &str,
) -> Result<Value, String> {
    let answer = pgcsv::query(prepared);
    let prompt = fs::read_to_string(prompt_path).unwrap_or_default();
    if !prompt.is_empty() && prepared["_host_expected"] != prompt {
        return Ok(json!({"host_confirmation":prompt}));
    }
    let mut result = answer.map_err(|e| e.to_string())?;
    if action == "connect" && result["connected"] == true {
        let saved = (|| {
            let _guard = history::LOCK.lock().map_err(|_| "History lock failed")?;
            let path = history_path.ok_or("History folder unavailable")?;
            history::remember(path, prepared)
        })();
        if let Err(error) = saved {
            result["history_warning"] = json!(error);
        }
    }
    Ok(result)
}

#[tauri::command]
fn connection_history(
    app: tauri::AppHandle,
    action: String,
    value: Value,
) -> Result<Value, String> {
    let _guard = history::LOCK.lock().map_err(|_| "History lock failed")?;
    history::update(
        &store_path(&app)?.with_file_name("connection-history.json"),
        &action,
        &value,
    )
}

#[tauri::command]
async fn choose(app: tauri::AppHandle, kind: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = app.dialog().file();
        let path = match kind.as_str() {
            "output" => dialog
                .add_filter("CSV", &["csv"])
                .set_file_name("sensors.csv")
                .blocking_save_file(),
            "folder" => dialog.blocking_pick_folder(),
            "csv" => dialog
                .add_filter("CSV / TSV", &["csv", "tsv", "txt"])
                .blocking_pick_file(),
            "profile" => dialog.add_filter("JSON", &["json"]).blocking_pick_file(),
            "key" => dialog.blocking_pick_file(),
            _ => return Err("invalid dialog".into()),
        };
        path.map(|p| {
            p.into_path()
                .map(|p| p.to_string_lossy().into_owned())
                .map_err(|e| e.to_string())
        })
        .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn import_profile(path: String) -> Result<Value, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 65536 {
        return Err("設定ファイルが大きすぎます。".into());
    }
    let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    let mut profile: Value =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())?;
    profile = sanitized_profile(&profile)?;
    Ok(profile)
}

fn sanitized_profile(profile: &Value) -> Result<Value, String> {
    pgcsv::settings::sanitized_profile(profile)
}

fn store_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let folder = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    Ok(folder.join("profiles.json"))
}
fn read_profiles(app: &tauri::AppHandle) -> Result<Value, String> {
    pgcsv::settings::read(&store_path(app)?)
}
#[tauri::command]
fn profiles(app: tauri::AppHandle) -> Result<Value, String> {
    read_profiles(&app)
}
#[tauri::command]
fn load_profile(app: tauri::AppHandle, name: String) -> Result<Value, String> {
    pgcsv::settings::load(&store_path(&app)?, &name)
}
#[tauri::command]
fn save_profile(
    app: tauri::AppHandle,
    name: String,
    profile: Value,
    save_secrets: bool,
) -> Result<(), String> {
    pgcsv::settings::save(&store_path(&app)?, &name, &profile, save_secrets, true)
}
#[tauri::command]
fn delete_profile(app: tauri::AppHandle, name: String) -> Result<(), String> {
    pgcsv::settings::remove(&store_path(&app)?, &name)
}
#[tauri::command]
fn language_preference(value: Option<String>) -> Result<Option<String>, String> {
    if let Some(v) = value {
        pgcsv::settings::set_language(&v)?;
    }
    Ok(pgcsv::settings::stored_language())
}

#[tauri::command]
fn destination(output: String) -> Result<Value, String> {
    let path = PathBuf::from(output);
    let parent = if path.is_dir() {
        path.as_path()
    } else {
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or("保存先を指定してください。")?
    };
    let temp = tempfile::NamedTempFile::new_in(parent).map_err(|_| "保存先に書き込めません。")?;
    drop(temp);
    Ok(
        json!({"exists":path.exists(),"free_bytes":fs2::available_space(parent).map_err(|e|e.to_string())?}),
    )
}
#[tauri::command]
fn open_output(state: State<Session>) -> Result<(), String> {
    let value = state.result.lock().map_err(|_| "state lock")?;
    let output = value
        .as_ref()
        .filter(|v| v["status"] == "complete")
        .and_then(|v| v["result"]["output"].as_str())
        .ok_or("完了したCSVがありません。")?;
    let path = std::path::Path::new(output);
    let parent = if path.is_dir() {
        path
    } else {
        path.parent().ok_or("保存先がありません。")?
    };
    std::process::Command::new("explorer.exe")
        .arg(parent)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn begin_export(
    app: tauri::AppHandle,
    state: State<Session>,
    profile: Value,
    output: String,
    overwrite: bool,
    split_options: Option<Value>,
) -> Result<(), String> {
    let mut active = state.cancel.lock().map_err(|_| "state lock")?;
    if active.is_some() {
        return Err("実行中です。".into());
    }
    let flag = Arc::new(AtomicBool::new(false));
    *active = Some(flag.clone());
    state.received.store(0, Ordering::Relaxed);
    let received = state.received.clone();
    *state.result.lock().map_err(|_| "state lock")? = None;
    drop(active);
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| {
            let temp = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
            let mut profile = prepare(profile, temp.path())?;
            if let Some(options) = split_options {
                profile["_split"] = options;
            }
            pgcsv::export_progress(
                &profile,
                std::path::Path::new(&output),
                overwrite,
                flag,
                received,
            )
            .map_err(|e| e.to_string())
        })();
        let state = app.state::<Session>();
        *state.result.lock().unwrap() = Some(match result {
            Ok(value) => json!({"status":"complete","result":value}),
            Err(error) => json!({"status":"failed","error":error}),
        });
        *state.cancel.lock().unwrap() = None;
    });
    Ok(())
}
#[tauri::command]
fn csv_metadata(input: String) -> Result<Value, String> {
    let path = std::path::Path::new(&input);
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("Choose a regular CSV file".into());
    }
    Ok(json!({"name":path.file_name().unwrap_or_default().to_string_lossy(),"bytes":meta.len()}))
}
#[tauri::command]
fn cancel_csv_scan(state: State<Session>, id: u64) {
    state.scan.lock().unwrap().cancel(id);
}
#[tauri::command]
async fn scan_csv(
    state: State<'_, Session>,
    input: String,
    options: Value,
    id: u64,
) -> Result<Value, String> {
    let options = pgcsv::split::Options::from_value(&options).map_err(|e| e.to_string())?;
    let flag = state.scan.lock().map_err(|_| "state lock")?.begin(id)?;
    let gate = state.scan_gate.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _guard = gate.lock().map_err(|_| "state lock".to_string())?;
        if flag.load(Ordering::Relaxed) {
            return Err("Cancelled".to_string());
        }
        pgcsv::split::inspect_file(std::path::Path::new(&input), &options, &flag)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?;
    let mut active = state.scan.lock().map_err(|_| "state lock")?;
    if active.last_id == id {
        active.active = None;
    }
    result
}

#[tauri::command]
fn begin_split(
    app: tauri::AppHandle,
    state: State<Session>,
    input: String,
    output: String,
    options: Value,
) -> Result<(), String> {
    let options = pgcsv::split::Options::from_value(&options).map_err(|e| e.to_string())?;
    let mut active = state.cancel.lock().map_err(|_| "state lock")?;
    if active.is_some() {
        return Err("実行中です。".into());
    }
    let flag = Arc::new(AtomicBool::new(false));
    *active = Some(flag.clone());
    state.received.store(0, Ordering::Relaxed);
    let received = state.received.clone();
    *state.result.lock().map_err(|_| "state lock")? = None;
    drop(active);
    tauri::async_runtime::spawn_blocking(move || {
        let result = pgcsv::split::split_file(
            std::path::Path::new(&input),
            std::path::Path::new(&output),
            options,
            &flag,
            &received,
        );
        let state = app.state::<Session>();
        *state.result.lock().unwrap() = Some(match result {
            Ok(value) => json!({"status":"complete","result":value}),
            Err(e) => json!({"status":"failed","error":e.to_string()}),
        });
        *state.cancel.lock().unwrap() = None;
    });
    Ok(())
}
#[tauri::command]
fn status(state: State<Session>) -> Value {
    json!({"running":state.cancel.lock().unwrap().is_some(),"received_bytes":state.received.load(Ordering::Relaxed),"completion":state.result.lock().unwrap().clone()})
}
#[tauri::command]
fn cancel(state: State<Session>) {
    if let Some(flag) = state.cancel.lock().unwrap().as_ref() {
        flag.store(true, Ordering::Relaxed);
    }
}

fn main() {
    let _runtime_guard =
        pgcsv::settings::runtime_lock(false).expect("Update in progress; retry after it finishes");
    tauri::Builder::default()
        .manage(Session::default())
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.state::<Session>().cancel.lock().unwrap().is_some()
            {
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            inspect,
            open_error_help,
            connection_history,
            choose,
            import_profile,
            profiles,
            language_preference,
            load_profile,
            save_profile,
            delete_profile,
            destination,
            open_output,
            begin_export,
            begin_split,
            csv_metadata,
            scan_csv,
            cancel_csv_scan,
            status,
            cancel
        ])
        .run(tauri::generate_context!())
        .expect("OURS Data Converter could not start");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scan_ids_reject_late_start_and_preserve_newer_work() {
        let mut scan = Scan::default();
        scan.cancel(1);
        assert!(scan.begin(1).is_err());
        let second = scan.begin(2).unwrap();
        let third = scan.begin(3).unwrap();
        assert!(second.load(Ordering::Relaxed));
        assert!(scan.begin(2).is_err());
        scan.cancel(2);
        assert!(!third.load(Ordering::Relaxed));
        scan.cancel(3);
        assert!(third.load(Ordering::Relaxed));
        assert!(scan.begin(3).is_err());
        assert!(scan.begin(4).is_ok());
    }
    #[test]
    #[ignore = "requires explicit read-only real-server profile"]
    fn real_ssh_history_environment_and_export_integration() {
        let path = std::env::var("PGCSV_INTEGRATION_PROFILE").expect("profile path required");
        let mut p: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let d = tempfile::tempdir().unwrap();
        let prompt = d.path().join("prompt");
        fs::write(&prompt, "").unwrap();
        let hist = d.path().join("history.json");
        p["action"] = json!("connect");
        let result = inspect_prepared(&p, &prompt, Some(&hist), "connect").unwrap();
        assert_eq!(result["connected"], true);
        assert_eq!(
            history::read(&hist).unwrap()["entries"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let no_history = inspect_prepared(&p, &prompt, None, "connect").unwrap();
        assert_eq!(no_history["connected"], true);
        assert!(no_history["history_warning"].is_string());
        let corrupt = d.path().join("corrupt-history.json");
        fs::write(&corrupt, "invalid").unwrap();
        let unavailable = inspect_prepared(&p, &prompt, Some(&corrupt), "connect").unwrap();
        assert_eq!(unavailable["connected"], true);
        assert!(unavailable["history_warning"].is_string());
        let refused = d.path().join("refused-history.json");
        let mut invalid = p.clone();
        invalid["ssh_host"] = json!("-invalid");
        assert!(inspect_prepared(&invalid, &prompt, Some(&refused), "connect").is_err());
        assert!(!refused.exists());
        let mut bad = p.clone();
        bad["remote_python"] = json!("/missing-pgcsv-python");
        bad["action"] = json!("ssh");
        assert!(inspect_prepared(&bad, &prompt, Some(&hist), "ssh").is_ok());
        assert_eq!(
            history::read(&hist).unwrap()["entries"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        p["action"] = json!("ssh");
        inspect_prepared(&p, &prompt, Some(&hist), "ssh").unwrap();
        p["action"] = json!("preview");
        let preview = inspect_prepared(&p, &prompt, Some(&hist), "preview").unwrap();
        assert!(preview["rows"].is_array());
        p.as_object_mut().unwrap().remove("action");
        let full = d.path().join("full.csv");
        let unsplit = pgcsv::export(&p, &full, false, Arc::new(AtomicBool::new(false))).unwrap();
        assert!(unsplit["rows"].as_u64().unwrap() > 0);
        let options=pgcsv::split::Options::from_value(&json!({"limit":100000,"header":true,"count_header":true,"encoding":"auto","delimiter":","})).unwrap();
        let split = pgcsv::split::split_file(
            &full,
            d.path(),
            options,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap();
        assert_eq!(split["rows"], unsplit["rows"]);
        p["_split"] = json!({"limit":100000,"header":true,"count_header":true,"encoding":"utf8","delimiter":","});
        let stream = pgcsv::export(&p, d.path(), false, Arc::new(AtomicBool::new(false))).unwrap();
        assert_eq!(stream["rows"], unsplit["rows"]);
        assert_eq!(stream["files"], split["files"]);
        // Independent parser verifies headers and every field in order against the unsplit output.
        let mut full_reader = csv_records(&full);
        let header = full_reader.headers().unwrap().clone();
        let expected = full_reader
            .records()
            .map(|r| r.unwrap())
            .collect::<Vec<_>>();
        for output in [&split, &stream] {
            let mut files = fs::read_dir(output["output"].as_str().unwrap())
                .unwrap()
                .map(|r| r.unwrap().path())
                .filter(|p| p.extension().is_some_and(|x| x == "csv"))
                .collect::<Vec<_>>();
            files.sort();
            let mut rows = Vec::new();
            for f in files {
                let mut reader = csv_records(&f);
                assert_eq!(reader.headers().unwrap(), &header);
                rows.extend(reader.records().map(|r| r.unwrap()));
            }
            assert_eq!(rows, expected);
        }
        println!(
            "real integration: rows={}, files={}, SSH/history/environment/preview/unsplit/local split/stream split/full ordered field comparison passed",
            unsplit["rows"], stream["files"]
        );
    }
    fn csv_records(path: &std::path::Path) -> csv::Reader<fs::File> {
        csv::Reader::from_path(path).unwrap()
    }
    #[test]
    fn split_profile_preserves_only_valid_export_options() {
        let p = json!({"split_options":{"limit":100,"header":true,"count_header":true,"delimiter":",","encoding":"utf8","password":"secret"}});
        let clean = sanitized_profile(&p).unwrap();
        assert_eq!(clean["split_options"]["limit"], 100);
        assert!(clean["split_options"].get("password").is_none());
        assert!(sanitized_profile(&json!({"split_options":{"limit":1,"header":true,"count_header":true,"delimiter":",","encoding":"utf8"}})).is_err());
    }
    #[test]
    fn profile_import_accepts_small_and_rejects_oversize_files() {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"{\"ssh_host\":\"example\"}").unwrap();
        assert_eq!(
            import_profile(file.path().to_string_lossy().into_owned()).unwrap()["ssh_host"],
            "example"
        );
        file.as_file().set_len(1024 * 1024).unwrap();
        assert!(
            import_profile(file.path().to_string_lossy().into_owned())
                .unwrap_err()
                .contains("大きすぎ")
        );
    }
    #[test]
    fn stored_settings_exclude_both_credentials_and_transient_host_approval() {
        let source = json!({"ssh_host":"server", "auth_mode":"password", "_ssh_secret":"ssh-secret", "_host_expected":"approved", "database":{"password":"nested"}, "unknown":{"password":"nested"}, "_database":{"host":"localhost", "user":"readonly", "password":"db-secret", "options":"unknown"}});
        let saved = sanitized_profile(&source).unwrap();
        assert_eq!(
            saved,
            json!({"ssh_host":"server", "auth_mode":"password", "_database":{"host":"localhost", "user":"readonly"}})
        );
        assert!(!saved.to_string().contains("secret"));
        assert!(sanitized_profile(&json!([])).is_err());
    }
}
