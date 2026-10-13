use crate::{Result, cli::message, settings};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::Path,
    time::Duration,
};
const API: &str = "https://api.github.com/repos/ours-labs/data-converter/releases/latest";
const PREFIX: &str = "https://github.com/ours-labs/data-converter/releases/download/";
const MAX_SIZE: u64 = 256 * 1024 * 1024;
#[derive(Debug)]
struct Release {
    version: semver::Version,
    url: String,
    size: u64,
    digest: String,
}
fn release(v: &Value) -> Result<Release> {
    if v["draft"] != false || v["prerelease"] != false {
        return Err("Only published stable releases are accepted".into());
    }
    let tag = v["tag_name"].as_str().ok_or("Missing version")?;
    let version = semver::Version::parse(tag.trim_start_matches('v'))?;
    if !version.pre.is_empty() {
        return Err("Prereleases are not update targets".into());
    }
    let name = format!("OURS-Data-Converter_{version}_x64-setup.exe");
    let matches: Vec<_> = v["assets"]
        .as_array()
        .ok_or("Missing assets")?
        .iter()
        .filter(|a| a["name"] == name)
        .collect();
    if matches.len() != 1 {
        return Err(
            "Stable release has no unique Windows installer / 安定版インストーラー未提供".into(),
        );
    }
    let a = matches[0];
    let url = a["browser_download_url"].as_str().ok_or("Missing URL")?;
    if url != format!("{PREFIX}{tag}/{name}") {
        return Err("Unexpected installer URL".into());
    }
    let digest = a["digest"]
        .as_str()
        .and_then(|s| s.strip_prefix("sha256:"))
        .ok_or("Missing SHA-256; refusing update")?;
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid SHA-256".into());
    }
    let size = a["size"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= MAX_SIZE)
        .ok_or("Invalid installer size")?;
    Ok(Release {
        version,
        url: url.into(),
        size,
        digest: digest.to_lowercase(),
    })
}
fn verify(path: &Path, size: u64, digest: &str) -> Result<()> {
    let mut file = std::fs::File::open(path)?;
    if file.metadata()?.len() != size {
        return Err("Installer size mismatch".into());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    if format!("{:x}", hash.finalize()) != digest {
        return Err("Installer SHA-256 mismatch".into());
    }
    Ok(())
}
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(120)))
        .user_agent("OURS-Data-Converter")
        .build()
        .into()
}
pub(crate) fn run(check: bool, yes: bool, interactive: bool) -> Result<()> {
    if !cfg!(windows) {
        return Err("Installer updates require Windows x64".into());
    }
    let client = agent();
    let response = client.get(API).call().map_err(|e| {
        format!("Update information unavailable (not a latest-version confirmation): {e}")
    })?;
    let mut data = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut data)?;
    if data.len() > 2 * 1024 * 1024 {
        return Err("Release metadata too large".into());
    }
    let latest = release(&serde_json::from_slice(&data)?)?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;
    println!(
        "{}: {current}\n{}: {}",
        message("Installed", "現在"),
        message("Stable", "安定版"),
        latest.version
    );
    if latest.version <= current {
        println!(
            "{}",
            message("No newer stable version", "新しい安定版はありません")
        );
        return Ok(());
    }
    if check {
        return Ok(());
    }
    let guard = settings::runtime_lock(true)?;
    if !yes
        && !crate::cli::confirm(
            "Download and start installer? / ダウンロードして更新しますか",
            interactive,
        )?
    {
        return Ok(());
    }
    let temp = tempfile::Builder::new().prefix("odc-update-").tempdir()?;
    let installer = temp.path().join("setup.exe");
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&installer)?;
    let mut reader = client
        .get(&latest.url)
        .call()?
        .into_body()
        .into_reader()
        .take(latest.size + 1);
    let copied = std::io::copy(&mut reader, &mut file)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    if copied != latest.size {
        return Err("Installer download size mismatch".into());
    }
    verify(&installer, latest.size, &latest.digest)?;
    let helper = temp.path().join("update-helper.exe");
    std::fs::copy(std::env::current_exe()?, &helper)?;
    let retained = temp.keep();
    let mut cmd = std::process::Command::new(helper);
    cmd.arg("__install_verified")
        .arg(&installer)
        .arg(latest.size.to_string())
        .arg(&latest.digest);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let child = cmd.spawn();
    drop(guard);
    child?;
    println!(
        "Installer handoff started; completion is not yet confirmed. / インストーラーへ引き渡しました。更新完了は次回 odc --version で確認してください。\nUpdate files / 更新ファイル: {}",
        retained.display()
    );
    Ok(())
}
pub(crate) fn install(args: &[String]) -> Result<()> {
    if args.len() != 3 {
        return Err("Invalid updater handoff".into());
    }
    let installer = Path::new(&args[0]);
    let dir = std::env::current_exe()?
        .parent()
        .ok_or("No helper folder")?
        .to_owned();
    if installer != dir.join("setup.exe")
        || !dir
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with("odc-update-"))
    {
        return Err("Invalid installer path".into());
    }
    let mut attempt = 0;
    let guard = loop {
        match settings::runtime_lock(true) {
            Ok(lock) => break lock,
            Err(e) => {
                attempt += 1;
                if attempt >= 20 {
                    return Err(e.into());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    };
    let result = (|| -> Result<std::process::ExitStatus> {
        verify(installer, args[1].parse()?, &args[2])?;
        Ok(std::process::Command::new(installer).arg("/P").status()?)
    })();
    if let Err(error) = &result {
        std::fs::write(dir.join("result.txt"), format!("Update failed: {error}\n"))?;
    }
    let result = result?;
    std::fs::write(
        dir.join("result.txt"),
        format!("Installer exit: {:?}\n", result.code()),
    )?;
    drop(guard);
    if !result.success() {
        return Err("Installer did not complete successfully".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> Value {
        json!({"draft":false,"prerelease":false,"tag_name":"v9.0.0","assets":[{"name":"OURS-Data-Converter_9.0.0_x64-setup.exe","browser_download_url":format!("{PREFIX}v9.0.0/OURS-Data-Converter_9.0.0_x64-setup.exe"),"digest":format!("sha256:{}","a".repeat(64)),"size":3}]})
    }
    #[test]
    fn metadata_rejects_untrusted() {
        assert!(release(&fixture()).is_ok());
        for key in ["draft", "prerelease"] {
            let mut v = fixture();
            v[key] = json!(true);
            assert!(release(&v).is_err())
        }
        for key in ["digest", "size", "browser_download_url", "name"] {
            let mut v = fixture();
            v["assets"][0][key] = json!("bad");
            assert!(release(&v).is_err())
        }
    }
    #[test]
    fn corrupted_download_never_executes() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("setup.exe");
        std::fs::write(&p, b"abc").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"abc"));
        assert!(verify(&p, 3, &digest).is_ok());
        assert!(verify(&p, 4, &digest).is_err());
        assert!(verify(&p, 3, &"a".repeat(64)).is_err());
    }
}
