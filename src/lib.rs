pub mod cli;
mod native;
pub mod settings;
pub mod split;
mod update;

use serde_json::{Value, json};
#[cfg(test)]
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};
#[cfg(test)]
use std::{
    io::{BufRead, BufReader, BufWriter},
    time::Instant,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

pub(crate) fn ssh_base(profile: &Value) -> Result<Command> {
    let host = profile["ssh_host"].as_str().ok_or("ssh_host missing")?;
    if host.starts_with('-')
        || host.is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.@:%-".contains(&b))
    {
        return Err("invalid ssh_host".into());
    }
    let mut command = Command::new("ssh");
    let interactive = profile.get("_askpass").is_some();
    command.args([
        "-T",
        "-o",
        if interactive {
            "BatchMode=no"
        } else {
            "BatchMode=yes"
        },
        "-o",
        if interactive {
            "StrictHostKeyChecking=ask"
        } else {
            "StrictHostKeyChecking=yes"
        },
        "-o",
        "ConnectTimeout=15",
        "-o",
        "NumberOfPasswordPrompts=1",
    ]);
    match profile["auth_mode"].as_str() {
        Some("password") => {
            command.args([
                "-o",
                "PubkeyAuthentication=no",
                "-o",
                "PreferredAuthentications=password,keyboard-interactive",
            ]);
        }
        Some("key") => {
            command.args([
                "-o",
                "IdentitiesOnly=yes",
                "-o",
                "PreferredAuthentications=publickey",
            ]);
        }
        _ => {}
    }
    if let Some(key) = profile["identity_file"].as_str().filter(|s| !s.is_empty()) {
        command.args(["-i", key]);
    }
    if let Some(port) = profile["ssh_port"].as_u64() {
        if port == 0 || port > 65535 {
            return Err("invalid SSH port".into());
        }
        command.args(["-p", &port.to_string()]);
    }
    if let Some(helper) = profile["_askpass"].as_str() {
        command
            .env("SSH_ASKPASS", helper)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env("DISPLAY", "pgcsv")
            .env("PGCSV_ASKPASS", "1")
            .env(
                "PGCSV_SSH_SECRET",
                profile["_ssh_secret"].as_str().unwrap_or(""),
            )
            .env(
                "PGCSV_HOST_PROMPT",
                profile["_host_prompt_path"].as_str().unwrap_or(""),
            )
            .env(
                "PGCSV_HOST_EXPECTED",
                profile["_host_expected"].as_str().unwrap_or(""),
            );
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }

    Ok(command)
}

fn ssh_command(profile: &Value) -> Result<Command> {
    let mut command = ssh_base(profile)?;
    command
        .arg(profile["ssh_host"].as_str().ok_or("ssh_host missing")?)
        .arg("cat >/dev/null; printf '{\"connected\":true}\\n'");
    Ok(command)
}

fn validate(profile: &Value) -> Result<()> {
    let object = profile.as_object().ok_or("profile must be an object")?;
    for forbidden in ["password", "dsn", "database_url", "private_key"] {
        if object.contains_key(forbidden) {
            return Err("secrets must remain on the server".into());
        }
    }
    if let Some(columns) = profile.get("columns") {
        let cols = columns.as_array().ok_or("columns must be an array")?;
        if cols.is_empty() || cols.iter().any(|c| c.as_str().is_none()) {
            return Err("invalid columns".into());
        }
        let mut seen = std::collections::HashSet::new();
        if !cols.iter().all(|c| seen.insert(c.as_str().unwrap())) {
            return Err("duplicate columns".into());
        }
    }
    Ok(())
}

pub fn askpass() -> bool {
    if std::env::var("PGCSV_ASKPASS").as_deref() != Ok("1") {
        return false;
    }
    let prompt = std::env::args().nth(1).unwrap_or_default();
    if prompt.contains("yes/no") || prompt.contains("Are you sure") {
        let path = std::env::var("PGCSV_HOST_PROMPT").unwrap_or_default();
        if !path.is_empty() {
            let _ = fs::write(path, &prompt);
        }
        let expected = std::env::var("PGCSV_HOST_EXPECTED").unwrap_or_default();
        println!(
            "{}",
            if !expected.is_empty() && expected == prompt {
                "yes"
            } else {
                "no"
            }
        );
    } else {
        println!("{}", std::env::var("PGCSV_SSH_SECRET").unwrap_or_default());
    }
    true
}

fn payload(profile: &Value) -> Value {
    if profile["action"] == "connect" {
        return json!({});
    }
    let mut result = profile.clone();
    result.as_object_mut().unwrap().retain(|key, _| {
        !key.starts_with('_')
            && !["ssh_host", "ssh_port", "identity_file", "remote_python"].contains(&key.as_str())
    });
    if let Some(database) = profile.get("_database") {
        result["database"] = database.clone();
    }
    result
}

pub fn query(profile: &Value) -> Result<Value> {
    validate(profile)?;
    if matches!(profile["action"].as_str(), Some("connect" | "ssh")) {
        query_command(&json!({"action":"connect"}), ssh_command(profile)?)
    } else {
        native::query(profile)
    }
}

fn bounded_output(
    mut stream: impl Read,
    limit: usize,
    child: Arc<Mutex<std::process::Child>>,
) -> std::result::Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = match stream.read(&mut buffer) {
            Ok(n) => n,
            Err(error) => {
                if let Ok(mut process) = child.lock() {
                    let _ = process.kill();
                }
                return Err(error.to_string());
            }
        };
        if n == 0 {
            return Ok(bytes);
        }
        if bytes.len() + n > limit {
            if let Ok(mut process) = child.lock() {
                let _ = process.kill();
            }
            return Err("query response exceeds size limit; select fewer columns".into());
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
}

fn query_command(profile: &Value, mut command: Command) -> Result<Value> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut input = child.stdin.take().ok_or("stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("stdout unavailable")?;
    let stderr = child.stderr.take().ok_or("stderr unavailable")?;
    let child = Arc::new(Mutex::new(child));
    let output_child = child.clone();
    let error_child = child.clone();
    let output_reader =
        thread::spawn(move || bounded_output(stdout, 4 * 1024 * 1024, output_child));
    let error_reader = thread::spawn(move || bounded_output(stderr, 64 * 1024, error_child));
    let sent = input.write_all(&serde_json::to_vec(&payload(profile))?);
    drop(input);
    if sent.is_err() {
        let _ = child.lock().map_err(|_| "process lock")?.kill();
    }
    let started = std::time::Instant::now();
    let mut timed_out = false;
    let status = loop {
        if started.elapsed() > Duration::from_secs(60) {
            timed_out = true;
            let _ = child.lock().map_err(|_| "process lock")?.kill();
        }
        if let Some(status) = child.lock().map_err(|_| "process lock")?.try_wait()? {
            break status;
        }
        thread::sleep(Duration::from_millis(20));
    };
    let stdout = output_reader.join().map_err(|_| "output reader failed")??;
    let stderr = error_reader.join().map_err(|_| "error reader failed")??;
    if timed_out {
        return Err("SSH query timed out".into());
    }
    sent?;
    if !status.success() {
        // Only transport diagnostics and worker error types; worker never emits DSNs.
        return Err(String::from_utf8_lossy(&stderr)
            .chars()
            .take(2000)
            .collect::<String>()
            .into());
    }
    Ok(serde_json::from_slice(&stdout)?)
}

fn check_csv(path: &Path, metadata: &Value, start: &Value, cancel: &AtomicBool) -> Result<u64> {
    let mut file = fs::File::open(path)?;
    let mut bom = [0u8; 3];
    file.read_exact(&mut bom)?;
    if bom != [0xef, 0xbb, 0xbf] {
        return Err("UTF-8 BOM missing".into());
    }
    let mut reader = csv::ReaderBuilder::new().flexible(false).from_reader(file);
    let header = reader
        .headers()?
        .iter()
        .map(String::from)
        .collect::<Vec<_>>();
    if json!(header) != start["columns"] {
        return Err("header mismatch".into());
    }
    let mut rows = 0;
    for row in reader.records() {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        row?;
        rows += 1;
    }
    if Some(rows) != metadata["rows"].as_u64() {
        return Err("row count mismatch".into());
    }
    Ok(rows)
}

pub fn export(
    profile: &Value,
    output: &Path,
    overwrite: bool,
    cancel: Arc<AtomicBool>,
) -> Result<Value> {
    export_progress(
        profile,
        output,
        overwrite,
        cancel,
        Arc::new(AtomicU64::new(0)),
    )
}

pub fn export_progress(
    profile: &Value,
    output: &Path,
    overwrite: bool,
    cancel: Arc<AtomicBool>,
    received: Arc<AtomicU64>,
) -> Result<Value> {
    validate(profile)?;
    native::export(profile, output, overwrite, cancel, received)
}

#[cfg(test)]
fn export_with_command(
    profile: &Value,
    output: &Path,
    overwrite: bool,
    cancel: Arc<AtomicBool>,
    command: Command,
) -> Result<Value> {
    export_transport(
        profile,
        output,
        overwrite,
        cancel,
        command,
        Arc::new(AtomicU64::new(0)),
    )
}

#[cfg(test)]
fn export_transport(
    profile: &Value,
    output: &Path,
    overwrite: bool,
    cancel: Arc<AtomicBool>,
    mut command: Command,
    received: Arc<AtomicU64>,
) -> Result<Value> {
    let split_options = profile
        .get("_split")
        .or_else(|| profile.get("split_options"))
        .map(split::Options::from_value)
        .transpose()?;
    let mut splitter = split_options
        .map(|mut options| {
            options.header = true;
            options.delimiter = b',';
            options.encoding = "utf8".into();
            split::Splitter::new(output, profile["table"].as_str().unwrap_or("data"), options)
        })
        .transpose()?;
    if splitter.is_none() && output.exists() && !overwrite {
        return Err("destination already exists".into());
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = if splitter.is_none() {
        Some(
            tempfile::Builder::new()
                .prefix("pgcsv-")
                .suffix(".part")
                .tempfile_in(parent)?,
        )
    } else {
        None
    };
    let started = Instant::now();
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdout = child.stdout.take().ok_or("stdout unavailable")?;
    let stderr = child.stderr.take().ok_or("stderr unavailable")?;
    let mut input = child.stdin.take().ok_or("stdin unavailable")?;
    let payload = payload(profile);
    let child = Arc::new(Mutex::new(child));
    let worker = child.clone();
    let stopped = Arc::new(AtomicBool::new(false));
    let stop = stopped.clone();
    let cancelled = cancel.clone();
    let watcher = thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            if cancelled.load(Ordering::Relaxed) {
                if let Ok(mut process) = worker.lock() {
                    let _ = process.kill();
                }
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
    });
    let receiver = thread::spawn(move || {
        let mut events = Vec::new();
        let mut errors = Vec::new();
        for line in BufReader::new(stderr)
            .lines()
            .map_while(std::result::Result::ok)
        {
            if let Some(value) = line.strip_prefix("PGCSV ") {
                if let Ok(event) = serde_json::from_str::<Value>(value) {
                    events.push(event);
                }
            } else if errors.len() < 10 {
                errors.push(line.chars().take(500).collect::<String>());
            }
        }
        (events, errors)
    });
    let transferred = (|| -> Result<(u64, String)> {
        input.write_all(&serde_json::to_vec(&payload)?)?;
        drop(input);
        if let Some(splitter) = splitter.as_mut() {
            struct Meter<'a> {
                input: &'a mut dyn Read,
                hash: Sha256,
                size: u64,
                received: &'a AtomicU64,
            }
            impl Read for Meter<'_> {
                fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
                    let n = self.input.read(b)?;
                    self.hash.update(&b[..n]);
                    self.size += n as u64;
                    self.received.store(self.size, Ordering::Relaxed);
                    Ok(n)
                }
            }
            let mut meter = Meter {
                input: &mut stdout,
                hash: Sha256::new(),
                size: 0,
                received: &received,
            };
            splitter.consume(
                std::io::Cursor::new(vec![0xef, 0xbb, 0xbf]).chain(&mut meter),
                &cancel,
            )?;
            return Ok((meter.size, format!("{:x}", meter.hash.finalize())));
        }
        let mut writer =
            BufWriter::with_capacity(1024 * 1024, temporary.as_mut().unwrap().as_file_mut());
        writer.write_all(&[0xef, 0xbb, 0xbf])?;
        let mut buffer = vec![0; 1024 * 1024];
        let mut size = 0;
        let mut hash = Sha256::new();
        loop {
            let n = stdout.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            writer.write_all(&buffer[..n])?;
            hash.update(&buffer[..n]);
            size += n as u64;
            received.store(size, Ordering::Relaxed);
        }
        writer.flush()?;
        Ok((size, format!("{:x}", hash.finalize())))
    })();
    if transferred.is_err() {
        let _ = child.lock().map_err(|_| "process lock")?.kill();
    }
    let status = loop {
        if let Some(status) = child.lock().map_err(|_| "process lock")?.try_wait()? {
            break status;
        }
        thread::sleep(Duration::from_millis(20));
    };
    stopped.store(true, Ordering::Relaxed);
    watcher.join().map_err(|_| "cancel watcher failed")?;
    let (events, errors) = receiver.join().map_err(|_| "metadata reader failed")?;
    let result = (|| -> Result<Value> {
        let (size, digest) = transferred?;
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        if !status.success() {
            return Err(format!("remote export failed: {events:?}; {errors:?}").into());
        }
        let starts = events
            .iter()
            .filter(|e| e["event"] == "start")
            .collect::<Vec<_>>();
        let completes = events
            .iter()
            .filter(|e| e["event"] == "complete")
            .collect::<Vec<_>>();
        if starts.len() != 1 || completes.len() != 1 || events.iter().any(|e| e["event"] == "error")
        {
            return Err("missing remote completion".into());
        }
        let metadata = completes[0];
        if Some(size) != metadata["bytes"].as_u64() || metadata["sha256"] != digest {
            return Err("stream integrity mismatch".into());
        }
        if let Some(splitter) = splitter.as_ref() {
            let header = csv::ReaderBuilder::new()
                .has_headers(false)
                .from_reader(splitter.header.as_slice())
                .records()
                .next()
                .transpose()?;
            let columns = header
                .map(|r| r.iter().map(String::from).collect::<Vec<_>>())
                .unwrap_or_default();
            if json!(columns) != starts[0]["columns"]
                || Some(splitter.rows) != metadata["rows"].as_u64()
            {
                return Err("split header or row count mismatch".into());
            }
            let mut result = splitter.complete(&cancel)?;
            result["input_bytes"] = json!(size);
            result["sha256_without_bom"] = json!(digest);
            result["total_seconds"] = json!(started.elapsed().as_secs_f64());
            result["snapshot"] = starts[0]["snapshot"].clone();
            return Ok(result);
        }
        let temporary = temporary.take().unwrap();
        temporary.as_file().sync_all()?;
        let stream_seconds = started.elapsed().as_secs_f64();
        let rows = check_csv(temporary.path(), metadata, starts[0], &cancel)?;
        if overwrite {
            temporary.persist(output)?;
        } else {
            temporary.persist_noclobber(output)?;
        }
        Ok(
            json!({"rows":rows,"bytes":size+3,"sha256_without_bom":digest,"snapshot":starts[0]["snapshot"],"remote_stream_seconds":metadata["stream_seconds"],"transfer_and_save_seconds":stream_seconds,"total_seconds":started.elapsed().as_secs_f64(),"output":output}),
        )
    })();
    result.map_err(|e| match &splitter {
        Some(s) => s.incomplete_error(e).into(),
        None => e,
    })
}

pub fn run_cli() -> Result<()> {
    let _runtime = settings::runtime_lock(false)?;
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) == Some("split") {
        if args.len() != 4 {
            return Err("usage: pgcsv split INPUT.csv PARENT_FOLDER OPTIONS.json".into());
        }
        let options = serde_json::from_slice(&fs::read(&args[3])?)?;
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        ctrlc::set_handler(move || flag.store(true, Ordering::Relaxed))?;
        let result = split::split_file(
            Path::new(&args[1]),
            Path::new(&args[2]),
            split::Options::from_value(&options)?,
            &cancel,
            &AtomicU64::new(0),
        )?;
        println!("{result}");
        return Ok(());
    }
    if args.len() < 2 {
        return Err("usage: pgcsv PROFILE.json OUTPUT.csv [--overwrite]".into());
    }
    if args[2..].iter().any(|v| v != "--overwrite") {
        return Err("unknown option".into());
    }
    let text = fs::read_to_string(&args[0])?;
    let profile = serde_json::from_str(text.trim_start_matches('\u{feff}'))?;
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    ctrlc::set_handler(move || flag.store(true, Ordering::Relaxed))?;
    let result = export(
        &profile,
        Path::new(&args[1]),
        args.iter().any(|s| s == "--overwrite"),
        cancel,
    )?;
    println!("{result}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ssh_probe_is_fixed_and_does_not_send_database_credentials() {
        let p = json!({"ssh_host":"pi@example-host","action":"connect","remote_python":"not-a-path","_database":{"password":"never-send"}});
        let command = ssh_command(&p).unwrap();
        assert_eq!(
            command.get_args().last().unwrap(),
            "cat >/dev/null; printf '{\"connected\":true}\\n'"
        );
        assert_eq!(payload(&p), json!({}));
    }
    #[test]
    fn query_output_limits_terminate_children() {
        for (stream, size) in [("stdout", 4 * 1024 * 1024 + 1), ("stderr", 65537)] {
            let mut command = Command::new("python");
            command.args(["-c", &format!("import sys,time;sys.stdin.read();sys.{stream}.buffer.write(b'x'*{size});sys.{stream}.flush();time.sleep(5)")]);
            let start = Instant::now();
            let error = query_command(&json!({}), command).unwrap_err().to_string();
            assert!(error.contains("size limit"), "{error}");
            assert!(start.elapsed() < Duration::from_secs(3));
        }
        let mut command = Command::new("python");
        command.args(["-c", "import sys;sys.stdin.read();print('{\"rows\": []}')"]);
        assert_eq!(
            query_command(&json!({}), command).unwrap()["rows"],
            json!([])
        );
    }

    #[test]
    fn cancellation_after_stdout_eof_does_not_wait_for_exit() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("cancel.csv");
        fs::write(&path, b"previous").unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let setter = thread::spawn(move || {
            thread::sleep(Duration::from_millis(250));
            flag.store(true, Ordering::Relaxed);
        });
        let mut command = Command::new("python");
        command.args([
            "-c",
            "import sys,os,time;sys.stdin.read();os.close(1);time.sleep(5)",
        ]);
        let start = Instant::now();
        assert!(export_with_command(&json!({}), &path, true, cancel, command).is_err());
        setter.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        assert_eq!(fs::read(&path).unwrap(), b"previous");
        assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 1);
    }
    #[test]
    fn authentication_arguments_precede_host_and_secrets_use_stdin_or_environment() {
        let profile = json!({"ssh_host":"server", "remote_python":"/usr/bin/python3", "identity_file":"C:/keys/a b", "ssh_port":2222, "auth_mode":"key", "_askpass":"pgcsv.exe", "_ssh_secret":"ssh-secret", "_database":{"password":"db-secret"}});
        let command = ssh_command(&profile).unwrap();
        let args = command
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let host = args.iter().position(|s| s == "server").unwrap();
        assert_eq!(host, args.len() - 2);
        assert!(args[..host].iter().any(|s| s == "IdentitiesOnly=yes"));
        assert!(!format!("{args:?}").contains("secret"));
        let body = payload(&profile);
        assert_eq!(body["database"]["password"], "db-secret");
        assert!(body.get("_ssh_secret").is_none());
        assert!(body.get("_host_expected").is_none());
        assert!(body.get("identity_file").is_none());
    }
    fn fixture(
        data: &str,
        rows: u64,
        fail: bool,
        delay: f64,
        hash_override: Option<&str>,
    ) -> Command {
        let digest = hash_override
            .map(String::from)
            .unwrap_or_else(|| format!("{:x}", Sha256::digest(data.as_bytes())));
        let events = format!(
            "PGCSV {}\nPGCSV {}\n",
            json!({"event":"start","columns":["name","value"],"snapshot":"fixture"}),
            json!({"event":"complete","rows":rows,"bytes":data.len(),"sha256":digest,"stream_seconds":0.01})
        );
        let code = format!(
            "import sys,time;sys.stdin.read();time.sleep({delay});sys.stdout.buffer.write({}.encode());sys.stderr.write({});sys.exit({})",
            serde_json::to_string(data).unwrap(),
            serde_json::to_string(&events).unwrap(),
            u8::from(fail)
        );
        let mut command = Command::new("python");
        command.args(["-c", &code]);
        command
    }
    #[test]
    fn streamed_split_validates_remote_integrity_before_completion() {
        let folder = tempfile::tempdir().unwrap();
        let profile = json!({"table":"sensor","_split":{"limit":2,"header":true,"count_header":true,"delimiter":",","encoding":"utf8"}});
        let data = "name,value\n日本語,1\n空,2\n";
        let result = export_with_command(
            &profile,
            folder.path(),
            false,
            Arc::new(AtomicBool::new(false)),
            fixture(data, 2, false, 0.0, None),
        )
        .unwrap();
        assert_eq!(result["files"], 2);
        assert_eq!(result["rows"], 2);
        let out = Path::new(result["output"].as_str().unwrap());
        assert!(!out.join("INCOMPLETE.txt").exists());
        assert_eq!(
            fs::read(out.join("sensor_002.csv")).unwrap(),
            [
                vec![0xef, 0xbb, 0xbf],
                "name,value\n空,2\n".as_bytes().to_vec()
            ]
            .concat()
        );
        for (rows, fail, hash) in [(2, false, Some("wrong")), (3, false, None), (2, true, None)] {
            let err = export_with_command(
                &profile,
                folder.path(),
                false,
                Arc::new(AtomicBool::new(false)),
                fixture(data, rows, fail, 0.0, hash),
            )
            .unwrap_err()
            .to_string();
            assert!(err.contains("Incomplete folder:"), "{err}");
        }
        let incomplete = fs::read_dir(folder.path())
            .unwrap()
            .filter(|d| d.as_ref().unwrap().path().join("INCOMPLETE.txt").exists())
            .count();
        assert_eq!(incomplete, 3);
        let empty = export_with_command(
            &profile,
            folder.path(),
            false,
            Arc::new(AtomicBool::new(false)),
            fixture("name,value\n", 0, false, 0.0, None),
        )
        .unwrap();
        assert_eq!(empty["files"], 0);
    }
    #[test]
    fn transport_and_atomic_failure() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("日本語.csv");
        let data = "name,value\n日本語,1\n";
        let cancel = Arc::new(AtomicBool::new(false));
        let result = export_with_command(
            &json!({}),
            &path,
            false,
            cancel.clone(),
            fixture(data, 1, false, 0.0, None),
        )
        .unwrap();
        assert_eq!(result["rows"], 1);
        assert_eq!(
            fs::read(&path).unwrap(),
            [vec![0xef, 0xbb, 0xbf], data.as_bytes().to_vec()].concat()
        );
        let previous = fs::read(&path).unwrap();
        for (count, fail, hash) in [(1, true, None), (2, false, None), (1, false, Some("wrong"))] {
            assert!(
                export_with_command(
                    &json!({}),
                    &path,
                    true,
                    cancel.clone(),
                    fixture(data, count, fail, 0.0, hash)
                )
                .is_err()
            );
            assert_eq!(fs::read(&path).unwrap(), previous);
        }
        assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 1);
    }
    #[test]
    fn cancellation_stops_blocked_read() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("cancel.csv");
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let setter = thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            flag.store(true, Ordering::Relaxed);
        });
        assert!(
            export_with_command(
                &json!({}),
                &path,
                false,
                cancel,
                fixture("name,value\n", 0, false, 5.0, None)
            )
            .is_err()
        );
        setter.join().unwrap();
        assert!(!path.exists());
        assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 0);
    }
    #[test]
    fn reject_host_injection() {
        for host in ["-oProxyCommand=evil", "host; echo bad", "host\n"] {
            assert!(
                ssh_command(&json!({"ssh_host":host,"remote_python":"/usr/bin/python3"})).is_err()
            );
        }
    }
    #[test]
    fn reject_secrets_and_duplicate_columns() {
        assert!(validate(&json!({"password":"hidden"})).is_err());
        assert!(validate(&json!({"columns":["temp","temp"]})).is_err());
    }
    #[test]
    fn csv_japanese_quotes_newlines_and_empty() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all("\u{feff}name,value\n\"日本語,引用\"\"\n改行\",\n\"\",1\n".as_bytes())
            .unwrap();
        assert_eq!(
            check_csv(
                file.path(),
                &json!({"rows":2}),
                &json!({"columns":["name","value"]}),
                &AtomicBool::new(false)
            )
            .unwrap(),
            2
        );
    }
    #[test]
    fn refuse_invalid_csv_and_count() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"\xef\xbb\xbfa,b\n1\n").unwrap();
        assert!(
            check_csv(
                file.path(),
                &json!({"rows":1}),
                &json!({"columns":["a","b"]}),
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
}
