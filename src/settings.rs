use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
type Result<T, E = String> = std::result::Result<T, E>;
pub fn folder() -> Result<PathBuf> {
    #[cfg(windows)]
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or("APPDATA unavailable")?;
    #[cfg(not(windows))]
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        .ok_or("Config directory unavailable")?;
    let path = root.join("jp.kazuki.pgcsv");
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path)
}
pub fn lock(path: &Path, exclusive: bool) -> Result<fs::File> {
    fs::create_dir_all(path.parent().ok_or("Missing parent")?).map_err(|e| e.to_string())?;
    let f = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    let result = if exclusive {
        fs2::FileExt::try_lock_exclusive(&f)
    } else {
        fs2::FileExt::try_lock_shared(&f)
    };
    result
        .map_err(|_| "Application/settings busy; close other operations and retry".to_string())?;
    Ok(f)
}
pub fn runtime_lock(exclusive: bool) -> Result<fs::File> {
    lock(&folder()?.join("runtime.lock"), exclusive)
}
fn read_unlocked(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let data = fs::read(path).map_err(|e| e.to_string())?;
    if data.len() > 4 * 1024 * 1024 {
        return Err("Settings too large".into());
    }
    let v: Value = serde_json::from_slice(&data).map_err(|_| "Invalid settings JSON")?;
    if !v.is_object() {
        return Err("Settings must be an object".into());
    }
    Ok(v)
}
pub fn read(path: &Path) -> Result<Value> {
    let _lock = lock(&path.with_extension("lock"), true)?;
    read_unlocked(path)
}
fn write(path: &Path, value: &Value) -> Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing parent")?)
        .map_err(|e| e.to_string())?;
    temp.write_all(&serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
pub fn load(path: &Path, name: &str) -> Result<Value> {
    let _lock = lock(&path.with_extension("lock"), true)?;
    let mut p = read_unlocked(path)?[name].clone();
    if p.is_null() {
        return Err("Profile not found".into());
    }
    if p["saved_secrets"] == true {
        let entry = keyring::Entry::new(
            "PG CSV Exporter",
            p["credential_id"].as_str().unwrap_or(name),
        )
        .map_err(|e| e.to_string())?;
        let secret: Value = serde_json::from_str(
            &entry
                .get_password()
                .map_err(|_| "Saved credentials unavailable")?,
        )
        .map_err(|_| "Invalid saved credentials")?;
        p["_ssh_secret"] = secret["ssh"].clone();
        if p["_database"].is_object() {
            p["_database"]["password"] = secret["db"].clone();
        }
    }
    Ok(p)
}
pub fn save(path: &Path, name: &str, profile: &Value, secrets: bool, replace: bool) -> Result<()> {
    if name.trim().is_empty() || name.len() > 120 || name.chars().any(char::is_control) {
        return Err("Invalid profile name".into());
    }
    let _lock = lock(&path.with_extension("lock"), true)?;
    let mut all = read_unlocked(path)?;
    if !replace && all.get(name).is_some() {
        return Err("Profile already exists".into());
    }
    let mut p = sanitized_profile(profile)?;
    let old_id = all[name]["credential_id"]
        .as_str()
        .unwrap_or(name)
        .to_owned();
    let new_id = format!(
        "odc-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    );
    let entry = keyring::Entry::new("PG CSV Exporter", &new_id).map_err(|e| e.to_string())?;
    if secrets {
        entry
            .set_password(
                &json!({"ssh":profile["_ssh_secret"],"db":profile["_database"]["password"]})
                    .to_string(),
            )
            .map_err(|_| "Credential storage failed")?;
    }
    if secrets {
        p["credential_id"] = json!(new_id);
    }
    p["saved_secrets"] = json!(secrets);
    all[name] = p;
    if let Err(error) = write(path, &all) {
        if secrets {
            let _ = entry.delete_credential();
        }
        return Err(error);
    }
    let _ = keyring::Entry::new("PG CSV Exporter", &old_id).and_then(|e| e.delete_credential());
    Ok(())
}
pub fn remove(path: &Path, name: &str) -> Result<()> {
    let _lock = lock(&path.with_extension("lock"), true)?;
    let mut all = read_unlocked(path)?;
    let old_id = all[name]["credential_id"]
        .as_str()
        .unwrap_or(name)
        .to_owned();
    all.as_object_mut().unwrap().remove(name);
    write(path, &all)?;
    let _ = keyring::Entry::new("PG CSV Exporter", &old_id).and_then(|e| e.delete_credential());
    Ok(())
}
pub fn stored_language() -> Option<String> {
    folder()
        .ok()
        .and_then(|p| read(&p.join("preferences.json")).ok())
        .and_then(|v| v["language"].as_str().map(str::to_owned))
        .filter(|v| v == "ja" || v == "en")
}
pub fn language() -> String {
    stored_language().unwrap_or_else(|| "en".into())
}
pub fn set_language(value: &str) -> Result<()> {
    if !["en", "ja"].contains(&value) {
        return Err("Invalid language".into());
    }
    let p = folder()?.join("preferences.json");
    let _lock = lock(&p.with_extension("lock"), true)?;
    write(&p, &json!({"language":value}))
}
pub fn sanitized_profile(profile: &Value) -> Result<Value, String> {
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
    if let Some(value) = profile.get("split_options") {
        let options = crate::split::Options::from_value(value).map_err(|e| e.to_string())?;
        object.insert("split_options".into(), json!({"limit":options.limit,"header":true,"count_header":options.count_header,"delimiter":",","encoding":"utf8"}));
    }
    Ok(Value::Object(object))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secret_fields_never_reach_profile_json() {
        let value=sanitized_profile(&json!({"ssh_host":"example","_ssh_secret":"sentinel","credential_id":"untrusted","_database":{"host":"localhost","password":"sentinel"},"extra":"sentinel"})).unwrap();
        assert!(!value.to_string().contains("sentinel"));
        assert!(value.get("credential_id").is_none());
    }
    #[test]
    fn profile_lock_and_atomic_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        write(&path, &json!({"one":{"table":"first"}})).unwrap();
        let held = lock(&path.with_extension("lock"), true).unwrap();
        assert!(read(&path).is_err());
        drop(held);
        assert_eq!(read(&path).unwrap()["one"]["table"], "first");
        write(&path, &json!({"one":{},"日本語":{}})).unwrap();
        assert_eq!(read(&path).unwrap().as_object().unwrap().len(), 2);
        fs::write(&path, "broken").unwrap();
        assert!(read(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "broken");
    }
    #[test]
    fn runtime_readers_block_update() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runtime.lock");
        let reader = lock(&path, false).unwrap();
        let second = lock(&path, false).unwrap();
        assert!(lock(&path, true).is_err());
        drop(reader);
        drop(second);
        let writer = lock(&path, true).unwrap();
        assert!(lock(&path, false).is_err());
        drop(writer);
        assert!(lock(&path, false).is_ok());
    }
}
