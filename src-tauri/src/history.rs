use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
pub static LOCK: Mutex<()> = Mutex::new(());
fn entry(p: &Value) -> Result<Value, String> {
    let host = p["ssh_host"].as_str().ok_or("Invalid history host")?;
    if host.is_empty()
        || host.len() > 512
        || host.starts_with('-')
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.@:%-".contains(&b))
    {
        return Err("Invalid history host".into());
    }
    let auth = p["auth_mode"].as_str().unwrap_or("existing");
    if !["existing", "key", "password"].contains(&auth) {
        return Err("Invalid history authentication".into());
    }
    let mut out = json!({"ssh_host":host,"auth_mode":auth});
    if let Some(v) = p.get("ssh_port") {
        let n = v
            .as_u64()
            .filter(|n| *n > 0 && *n <= 65535)
            .ok_or("Invalid history port")?;
        out["ssh_port"] = json!(n);
    }
    if auth == "key" {
        let key = p["identity_file"]
            .as_str()
            .ok_or("Invalid history key path")?;
        if key.is_empty() || key.len() > 4096 || key.chars().any(char::is_control) {
            return Err("Invalid history key path".into());
        }
        out["identity_file"] = json!(key);
    }
    Ok(out)
}
pub fn read(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!({"enabled":true,"entries":[]}));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "History unavailable")?
        .take(131073)
        .read_to_end(&mut bytes)
        .map_err(|_| "History unavailable")?;
    if bytes.len() > 131072 {
        return Err("History is corrupt; clear history to recover".into());
    }
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "History is corrupt; clear history to recover")?;
    let enabled = v["enabled"].as_bool().ok_or("Invalid history preference")?;
    let rows = v["entries"]
        .as_array()
        .filter(|a| a.len() <= 20)
        .ok_or("Invalid history entries")?;
    let mut clean = Vec::new();
    for row in rows {
        let mut item = entry(row)?;
        item["last_success"] = json!(row["last_success"].as_u64().ok_or("Invalid history date")?);
        if &item != row {
            return Err("Unexpected history fields; clear history to recover".into());
        }
        clean.push(item);
    }
    Ok(json!({"enabled":enabled,"entries":clean}))
}
fn write(path: &Path, v: &Value) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid history path")?;
    fs::create_dir_all(parent).map_err(|_| "History save failed")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|_| "History save failed")?;
    temp.write_all(&serde_json::to_vec(v).map_err(|_| "History save failed")?)
        .map_err(|_| "History save failed")?;
    temp.as_file()
        .sync_all()
        .map_err(|_| "History save failed")?;
    temp.persist(path).map_err(|_| "History save failed")?;
    Ok(())
}
pub fn remember(path: &Path, profile: &Value) -> Result<(), String> {
    let mut v = read(path)?;
    if v["enabled"] == false {
        return Ok(());
    }
    let mut item = entry(profile)?;
    let rows = v["entries"].as_array_mut().unwrap();
    rows.retain(|old| {
        let mut old = old.clone();
        old.as_object_mut().unwrap().remove("last_success");
        old != item
    });
    item["last_success"] = json!(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "Invalid clock")?
            .as_secs()
    );
    rows.insert(0, item);
    rows.truncate(20);
    write(path, &v)
}
pub fn update(path: &Path, action: &str, value: &Value) -> Result<Value, String> {
    let mut v = if action == "clear" {
        let enabled = read(path)
            .ok()
            .and_then(|v| v["enabled"].as_bool())
            .unwrap_or(true);
        json!({"enabled":enabled,"entries":[]})
    } else {
        read(path)?
    };
    match action {
        "read" => return Ok(v),
        "clear" => {}
        "enabled" => {
            v["enabled"] = json!(value.as_bool().ok_or("Invalid history preference")?);
        }
        "delete" => {
            let rows = v["entries"].as_array_mut().unwrap();
            rows.retain(|r| r != value);
        }
        _ => return Err("Invalid history action".into()),
    };
    write(path, &v)?;
    Ok(v)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistence_bounds_secrets_and_preferences() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("history.json");
        for i in 0..21 {
            remember(&p,&json!({"ssh_host":format!("pi@host{i}"),"_ssh_secret":"NEVER","password":"NEVER","_database":{"password":"NEVER"}})).unwrap();
        }
        let v = read(&p).unwrap();
        assert_eq!(v["entries"].as_array().unwrap().len(), 20);
        assert_eq!(v["entries"][0]["ssh_host"], "pi@host20");
        assert!(!fs::read_to_string(&p).unwrap().contains("NEVER"));
        remember(&p, &json!({"ssh_host":"pi@host1"})).unwrap();
        assert_eq!(read(&p).unwrap()["entries"][0]["ssh_host"], "pi@host1");
        update(&p, "enabled", &json!(false)).unwrap();
        remember(&p, &json!({"ssh_host":"ignored"})).unwrap();
        let v = read(&p).unwrap();
        assert_eq!(v["entries"][0]["ssh_host"], "pi@host1");
        update(&p, "delete", &v["entries"][0]).unwrap();
        assert_eq!(read(&p).unwrap()["entries"].as_array().unwrap().len(), 19);
        update(&p, "clear", &Value::Null).unwrap();
        assert_eq!(read(&p).unwrap(), json!({"enabled":false,"entries":[]}));
    }
    #[test]
    fn corruption_and_io_failures() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("history.json");
        fs::write(&p, "broken").unwrap();
        assert!(read(&p).is_err());
        assert!(remember(&p, &json!({"ssh_host":"host"})).is_err());
        update(&p, "clear", &Value::Null).unwrap();
        remember(
            &p,
            &json!({"ssh_host":"alias","auth_mode":"key","identity_file":"C:/日本語/a b"}),
        )
        .unwrap();
        assert_eq!(
            read(&p).unwrap()["entries"][0]["identity_file"],
            "C:/日本語/a b"
        );
        assert!(
            update(
                &d.path().join("missing/child/history.json"),
                "delete",
                &Value::Null
            )
            .is_ok()
        );
        assert!(update(&p.join("child"), "clear", &Value::Null).is_err());
    }
}
