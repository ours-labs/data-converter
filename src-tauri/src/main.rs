#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
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
async fn inspect(profile: Value, action: String) -> Result<Value, String> {
    if !["ssh", "databases", "list", "preview", "estimate"].contains(&action.as_str()) {
        return Err("invalid action".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let temp = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
        let mut prepared = prepare(profile, temp.path())?;
        prepared["action"] = json!(action);
        let answer = pgcsv::query(&prepared);
        let prompt = fs::read_to_string(temp.path()).unwrap_or_default();
        if !prompt.is_empty() && prepared["_host_expected"] != prompt {
            return Ok(json!({"host_confirmation":prompt}));
        }
        answer.map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
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
    let mut object = profile
        .as_object()
        .ok_or("設定はJSON objectにしてください。")?
        .clone();
    object.retain(|key, _| {
        [
            "ssh_host",
            "ssh_port",
            "auth_mode",
            "identity_file",
            "remote_python",
            "env_file",
            "database_env",
            "database_name",
            "schema",
            "table",
            "columns",
            "time_column",
            "start",
            "end",
            "timezone",
            "timeout_ms",
            "output_path",
            "saved_secrets",
        ]
        .contains(&key.as_str())
    });
    if let Some(database) = profile["_database"].as_object() {
        let mut database = database.clone();
        database.retain(|key, _| ["host", "port", "dbname", "user"].contains(&key.as_str()));
        object.insert("_database".into(), Value::Object(database));
    }
    Ok(Value::Object(object))
}

fn store_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let folder = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    Ok(folder.join("profiles.json"))
}
fn read_profiles(app: &tauri::AppHandle) -> Result<Value, String> {
    let path = store_path(app)?;
    if !path.exists() {
        return Ok(json!({}));
    }
    serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
#[tauri::command]
fn profiles(app: tauri::AppHandle) -> Result<Value, String> {
    read_profiles(&app)
}
#[tauri::command]
fn load_profile(app: tauri::AppHandle, name: String) -> Result<Value, String> {
    let mut profile = read_profiles(&app)?[&name].clone();
    if profile.is_null() {
        return Err("設定がありません。".into());
    }
    if profile["saved_secrets"] == true {
        let entry = keyring::Entry::new("PG CSV Exporter", &name).map_err(|e| e.to_string())?;
        let secret: Value = serde_json::from_str(
            &entry
                .get_password()
                .map_err(|_| "保存済み資格情報を取得できません。")?,
        )
        .map_err(|e| e.to_string())?;
        profile["_ssh_secret"] = secret["ssh"].clone();
        if profile["_database"].is_object() {
            profile["_database"]["password"] = secret["db"].clone();
        }
    }
    Ok(profile)
}
#[tauri::command]
fn save_profile(
    app: tauri::AppHandle,
    name: String,
    mut profile: Value,
    save_secrets: bool,
) -> Result<(), String> {
    if name.trim().is_empty() || name.len() > 120 {
        return Err("設定名を入力してください。".into());
    }
    let secrets = json!({"ssh":profile["_ssh_secret"],"db":profile["_database"]["password"]});
    profile = sanitized_profile(&profile)?;
    let entry = keyring::Entry::new("PG CSV Exporter", &name).map_err(|e| e.to_string())?;
    if save_secrets {
        entry
            .set_password(&secrets.to_string())
            .map_err(|_| "資格情報の保存に失敗しました。")?;
    } else {
        let _ = entry.delete_credential();
    }
    profile["saved_secrets"] = json!(save_secrets);
    let path = store_path(&app)?;
    let mut stored = read_profiles(&app)?;
    stored[&name] = profile;
    let mut temp =
        tempfile::NamedTempFile::new_in(path.parent().unwrap()).map_err(|e| e.to_string())?;
    use std::io::Write;
    temp.write_all(&serde_json::to_vec_pretty(&stored).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn delete_profile(app: tauri::AppHandle, name: String) -> Result<(), String> {
    let mut stored = read_profiles(&app)?;
    stored
        .as_object_mut()
        .ok_or("invalid settings")?
        .remove(&name);
    let path = store_path(&app)?;
    let mut temp =
        tempfile::NamedTempFile::new_in(path.parent().unwrap()).map_err(|e| e.to_string())?;
    use std::io::Write;
    temp.write_all(&serde_json::to_vec_pretty(&stored).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    let _ = keyring::Entry::new("PG CSV Exporter", &name).and_then(|e| e.delete_credential());
    Ok(())
}
#[tauri::command]
fn destination(output: String) -> Result<Value, String> {
    let path = PathBuf::from(output);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or("保存先を指定してください。")?;
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
    let parent = std::path::Path::new(output)
        .parent()
        .ok_or("保存先がありません。")?;
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
            let profile = prepare(profile, temp.path())?;
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
            choose,
            import_profile,
            profiles,
            load_profile,
            save_profile,
            delete_profile,
            destination,
            open_output,
            begin_export,
            status,
            cancel
        ])
        .run(tauri::generate_context!())
        .expect("PG CSV Exporter could not start");
}

#[cfg(test)]
mod tests {
    use super::*;
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
