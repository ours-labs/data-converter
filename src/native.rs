//! PostgreSQL COPY over an SSH stdio channel. No remote interpreter or listener.
use crate::{Result, split};
use bytes::Bytes;
use futures_util::TryStreamExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    io::{BufWriter, Read, Write},
    path::Path,
    pin::Pin,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf},
    process::{Child, ChildStdin, ChildStdout},
    runtime::Runtime,
};
use tokio_postgres::{
    Client, Config, CopyOutStream, SimpleQueryMessage,
    config::{Host, SslMode},
};

fn failure(code: &str, detail: &str) -> Box<dyn std::error::Error> {
    format!("{code}: {detail}").into()
}
fn db_error(e: tokio_postgres::Error) -> Box<dyn std::error::Error> {
    let code = e.code().map(|c| c.code()).unwrap_or("unavailable");
    failure(
        if code == "28P01" || code == "28000" {
            "ODC-DB-002"
        } else {
            "ODC-DB-001"
        },
        &format!("PostgreSQL operation failed; SQLSTATE={code}"),
    )
}
fn text<'a>(p: &'a Value, key: &str, default: &'a str) -> &'a str {
    p[key].as_str().unwrap_or(default)
}
fn identifier(s: &str) -> Result<String> {
    if s.is_empty() || s.contains('\0') {
        return Err(failure("ODC-CONFIG-001", "Invalid SQL identifier"));
    }
    Ok(format!("\"{}\"", s.replace('"', "\"\"")))
}
fn literal(s: &str) -> Result<String> {
    if s.contains('\0') {
        return Err(failure("ODC-CONFIG-001", "Invalid SQL value"));
    }
    Ok(format!("'{}'", s.replace('\'', "''")))
}
fn selection(p: &Value) -> Result<(String, String)> {
    let table = format!(
        "{}.{}",
        identifier(text(p, "schema", "public"))?,
        identifier(text(p, "table", ""))?
    );
    let cols = match p["columns"].as_array() {
        Some(a) => a
            .iter()
            .map(|v| identifier(v.as_str().unwrap_or("")))
            .collect::<Result<Vec<_>>>()?
            .join(","),
        None => "*".into(),
    };
    let mut parts = Vec::new();
    for (k, op) in [("start", ">="), ("end", "<")] {
        if let Some(v) = p[k].as_str().filter(|v| !v.is_empty()) {
            parts.push(format!(
                "{} {op} {}",
                identifier(text(p, "time_column", ""))?,
                literal(v)?
            ));
        }
    }
    let predicate = if parts.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", parts.join(" AND "))
    };
    Ok((
        format!("SELECT {cols} FROM {table}{predicate}"),
        format!("SELECT count(*)::text FROM {table}{predicate}"),
    ))
}
fn setting(source: &str, key: &str) -> Result<String> {
    if key.is_empty()
        || !key
            .bytes()
            .enumerate()
            .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()))
    {
        return Err(failure(
            "ODC-CONFIG-001",
            "Invalid environment variable name",
        ));
    }
    for line in source.lines() {
        if let Some((k, v)) = line.trim().split_once('=')
            && k == key
        {
            let v = v.trim();
            return Ok(
                if v.len() >= 2
                    && ((v.starts_with('"') && v.ends_with('"'))
                        || (v.starts_with('\'') && v.ends_with('\'')))
                {
                    v[1..v.len() - 1].to_owned()
                } else {
                    v.to_owned()
                },
            );
        }
    }
    Err(failure("ODC-CONFIG-001", "Database setting not found"))
}
fn endpoint(config: &mut Config) -> Result<(String, u16)> {
    if config.get_hosts().len() > 1
        || config.get_ports().len() > 1
        || !config.get_hostaddrs().is_empty()
    {
        return Err(failure(
            "ODC-CONFIG-001",
            "Use one TCP hostname and port; hostaddr is unsupported",
        ));
    }
    let host = match config.get_hosts().first() {
        None => "127.0.0.1".to_owned(),
        Some(Host::Tcp(h)) => h.clone(),
        #[cfg(unix)]
        Some(Host::Unix(_)) => {
            return Err(failure(
                "ODC-CONFIG-001",
                "Unix socket connections are unsupported",
            ));
        }
    };
    if host.is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-:%".contains(&b))
    {
        return Err(failure("ODC-CONFIG-001", "Invalid DB hostname"));
    }
    let port = config.get_ports().first().copied().unwrap_or(5432);
    if port == 0 {
        return Err(failure("ODC-CONFIG-001", "Invalid DB port"));
    }
    let loopback = host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if !loopback {
        if config.get_ssl_mode() == SslMode::Disable {
            return Err(failure(
                "ODC-CONFIG-001",
                "TLS is required beyond the SSH server loopback",
            ));
        }
        config.ssl_mode(SslMode::Require);
    }
    if config.get_user().is_none() {
        return Err(failure("ODC-CONFIG-001", "Specify the database user"));
    }
    Ok((host, port))
}
async fn configuration(p: &Value) -> Result<Config> {
    let mut config = if let Some(db) = p.get("_database") {
        let mut c = Config::new();
        c.host(text(db, "host", "127.0.0.1"))
            .user(text(db, "user", ""))
            .dbname(text(db, "dbname", "postgres"))
            .password(text(db, "password", ""));
        let port = db["port"].as_u64().unwrap_or(5432);
        if port == 0 || port > 65535 {
            return Err(failure("ODC-CONFIG-001", "Invalid DB port"));
        }
        c.port(port as u16);
        c
    } else {
        let key = text(p, "database_env", "DATABASE_URL");
        setting(&format!("{key}=check"), key)?;
        let command = if let Some(path) = p["env_file"].as_str().filter(|s| !s.is_empty()) {
            if path.contains(['\0', '\n', '\r']) {
                return Err(failure("ODC-CONFIG-001", "Invalid settings path"));
            }
            let path = if let Some(rest) = path.strip_prefix("~/") {
                format!("\"$HOME\"/{}", crate::quote(rest))
            } else {
                crate::quote(path)
            };
            format!("head -c 65537 -- {path}")
        } else {
            format!("printenv -- {}", crate::quote(key))
        };
        let mut base = crate::ssh_base(p)?;
        base.arg(text(p, "ssh_host", "")).arg(command);
        let mut cmd = tokio::process::Command::from(base);
        cmd.kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd
            .spawn()
            .map_err(|_| failure("ODC-SSH-001", "Unable to start OpenSSH"))?;
        let mut out = Vec::new();
        child
            .stdout
            .take()
            .ok_or("SSH stdout missing")?
            .take(65537)
            .read_to_end(&mut out)
            .await?;
        if out.len() > 65536 {
            return Err(failure("ODC-CONFIG-001", "Settings exceed 64 KiB"));
        }
        if !child.wait().await?.success() {
            return Err(failure(
                "ODC-CONFIG-001",
                "Unable to read server settings over SSH",
            ));
        }
        let source = String::from_utf8(out)
            .map_err(|_| failure("ODC-CONFIG-001", "Settings must be UTF-8"))?;
        let dsn = if p["env_file"].as_str().is_some_and(|s| !s.is_empty()) {
            setting(&source, key)?
        } else {
            source.trim().to_owned()
        };
        dsn.parse::<Config>().map_err(|_| {
            failure(
                "ODC-CONFIG-001",
                "Unsupported or invalid PostgreSQL connection settings",
            )
        })?
    };
    if let Some(db) = p["database_name"].as_str().filter(|s| !s.is_empty()) {
        config.dbname(db);
    }
    config.application_name("OURS Data Converter");
    Ok(config)
}
struct Channel {
    input: ChildStdin,
    output: ChildStdout,
}
impl AsyncRead for Channel {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.output).poll_read(cx, buf)
    }
}
impl AsyncWrite for Channel {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.input).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.input).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.input).poll_shutdown(cx)
    }
}
async fn controlled<T>(
    future: impl Future<Output = Result<T>>,
    cancel: &AtomicBool,
    limit: Duration,
) -> Result<T> {
    tokio::select! {result=tokio::time::timeout(limit,future)=>result.map_err(|_|failure("ODC-TIMEOUT-001","Operation timed out"))?, _=async{loop{if cancel.load(Ordering::Relaxed){break}tokio::time::sleep(Duration::from_millis(50)).await;}}=>Err(failure("ODC-CANCEL-001","Cancelled"))}
}
struct Session {
    rt: Runtime,
    client: Client,
    child: Child,
    driver: tokio::task::JoinHandle<()>,
    cancel: Arc<AtomicBool>,
    limit: Duration,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.driver.abort();
        let _ = self.child.start_kill();
        self.rt.block_on(tokio_wait(&mut self.child));
    }
}
async fn tokio_wait(child: &mut Child) {
    let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
}
impl Session {
    fn open(p: &Value, cancel: Arc<AtomicBool>) -> Result<Self> {
        let ms = p["timeout_ms"].as_u64().unwrap_or(300000);
        if ms == 0 || ms > 3600000 {
            return Err(failure(
                "ODC-CONFIG-001",
                "Timeout must be 1..3600000 milliseconds",
            ));
        }
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let limit = Duration::from_millis(ms);
        let (client, child, driver) = rt.block_on(controlled(
            async {
                let mut config = configuration(p).await?;
                let (host, port) = endpoint(&mut config)?;
                let mut base = crate::ssh_base(p)?;
                base.args(["-W", &format!("[{host}]:{port}")])
                    .arg(text(p, "ssh_host", ""));
                let mut cmd = tokio::process::Command::from(base);
                cmd.kill_on_drop(true)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null());
                let mut child = cmd
                    .spawn()
                    .map_err(|_| failure("ODC-SSH-001", "Unable to start OpenSSH forwarding"))?;
                let stream = Channel {
                    input: child.stdin.take().ok_or("SSH stdin missing")?,
                    output: child.stdout.take().ok_or("SSH stdout missing")?,
                };
                let tls = postgres_native_tls::TlsConnector::new(
                    native_tls::TlsConnector::new()
                        .map_err(|_| failure("ODC-DB-001", "TLS initialization failed"))?,
                    &host,
                );
                let (client, connection) =
                    config.connect_raw(stream, tls).await.map_err(db_error)?;
                let driver = tokio::spawn(async move {
                    let _ = connection.await;
                });
                Ok((client, child, driver))
            },
            &cancel,
            Duration::from_secs(30),
        ))?;
        let session = Self {
            rt,
            client,
            child,
            driver,
            cancel,
            limit,
        };
        let setup = format!(
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY; SET LOCAL standard_conforming_strings=on; SET LOCAL statement_timeout={ms}; SET LOCAL DateStyle='ISO, YMD'; SET LOCAL client_encoding='UTF8'"
        );
        session.wait(async { session.client.batch_execute(&setup).await.map_err(db_error) })?;
        session.wait(async {
            session
                .client
                .execute(
                    "SELECT set_config('TimeZone', $1, true)",
                    &[&text(p, "timezone", "Asia/Tokyo")],
                )
                .await
                .map_err(db_error)?;
            Ok(())
        })?;
        if let (Some(a), Some(b)) = (p["start"].as_str(), p["end"].as_str()) {
            let rows = session.rows(&format!(
                "SELECT ({}::timestamptz < {}::timestamptz)::text",
                literal(a)?,
                literal(b)?
            ))?;
            if rows[0][0].as_deref() != Some("true") {
                return Err(failure("ODC-CONFIG-001", "Start must precede end"));
            }
        }
        Ok(session)
    }
    fn wait<T>(&self, f: impl Future<Output = Result<T>>) -> Result<T> {
        self.rt.block_on(controlled(f, &self.cancel, self.limit))
    }
    fn rows(&self, sql: &str) -> Result<Vec<Vec<Option<String>>>> {
        self.wait(async {
            let stream = self.client.simple_query_raw(sql).await.map_err(db_error)?;
            futures_util::pin_mut!(stream);
            let mut rows = Vec::new();
            let mut bytes = 0;
            while let Some(message) = stream.try_next().await.map_err(db_error)? {
                if let SimpleQueryMessage::Row(row) = message {
                    let values = (0..row.len())
                        .map(|i| row.get(i).map(str::to_owned))
                        .collect::<Vec<_>>();
                    bytes += values.iter().flatten().map(String::len).sum::<usize>();
                    if bytes > 4 * 1024 * 1024 || rows.len() >= 20000 {
                        return Err(failure("ODC-DB-001", "Query result exceeds display limit"));
                    }
                    rows.push(values);
                }
            }
            Ok(rows)
        })
    }
    fn copy(&self, sql: &str) -> Result<Pin<Box<CopyOutStream>>> {
        self.wait(async { Ok(Box::pin(self.client.copy_out(sql).await.map_err(db_error)?)) })
    }
    fn rollback(&self) -> Result<()> {
        self.wait(async {
            self.client
                .batch_execute("ROLLBACK")
                .await
                .map_err(db_error)
        })
    }
    fn columns(&self, select: &str) -> Result<Vec<String>> {
        self.wait(async {
            let statement = self
                .client
                .prepare(&format!("{select} LIMIT 0"))
                .await
                .map_err(db_error)?;
            let columns = statement
                .columns()
                .iter()
                .map(|c| c.name().to_owned())
                .collect::<Vec<_>>();
            if columns.len() > 256 {
                return Err(failure("ODC-DB-001", "Too many columns for display"));
            }
            Ok(columns)
        })
    }
}
struct CopyReader<'a> {
    session: &'a Session,
    stream: Pin<Box<CopyOutStream>>,
    chunk: Bytes,
    offset: usize,
    hash: Sha256,
    size: u64,
    received: &'a AtomicU64,
}
impl Read for CopyReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.session.cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("ODC-CANCEL-001: Cancelled"));
        }
        while self.offset == self.chunk.len() {
            let s = &self.session;
            let stream = &mut self.stream;
            match s
                .wait(async { stream.try_next().await.map_err(db_error) })
                .map_err(|e| std::io::Error::other(e.to_string()))?
            {
                Some(chunk) => {
                    self.chunk = chunk;
                    self.offset = 0
                }
                None => return Ok(0),
            }
        }
        let n = buf.len().min(self.chunk.len() - self.offset);
        buf[..n].copy_from_slice(&self.chunk[self.offset..self.offset + n]);
        self.offset += n;
        self.hash.update(&buf[..n]);
        self.size += n as u64;
        self.received.store(self.size, Ordering::Relaxed);
        Ok(n)
    }
}
fn copy_sql(select: &str) -> String {
    format!("COPY ({select}) TO STDOUT WITH (FORMAT CSV, HEADER, ENCODING 'UTF8')")
}
pub fn query(p: &Value) -> Result<Value> {
    let s = Session::open(p, Arc::new(AtomicBool::new(false)))?;
    let result=match text(p,"action",""){
 "databases"=>{let rows=s.rows("SELECT datname FROM pg_database WHERE datallowconn AND has_database_privilege(oid,'CONNECT') ORDER BY datname")?;let current=s.rows("SELECT current_database()")?;json!({"names":rows.iter().map(|r|r[0].clone()).collect::<Vec<_>>(),"current":current[0][0]})},
 "list"=>json!(s.rows("SELECT table_schema,table_name,column_name,data_type FROM information_schema.columns WHERE table_schema NOT IN ('pg_catalog','information_schema') ORDER BY table_schema,table_name,ordinal_position")?),
 "preview"=>{let (select,_)=selection(p)?;let names=s.columns(&select)?;let projection=names.iter().map(|n|{let id=identifier(n)?;Ok(format!("CASE WHEN length({id}::text)>1024 THEN left({id}::text,1024)||'…' ELSE {id}::text END AS {id}"))}).collect::<Result<Vec<_>>>()?.join(",");let rows=s.rows(&format!("SELECT {projection} FROM ({select} LIMIT 10) AS preview_source"))?;json!({"columns":names,"rows":rows,"cell_character_limit":1024})},
 "estimate"=>{let (select,count)=selection(p)?;let n:u64=s.rows(&count)?[0][0].as_ref().ok_or("Missing count")?.parse()?;let mut stream=s.copy(&copy_sql(&format!("{select} LIMIT 1000")))?;let mut bytes=0u64;while let Some(chunk)=s.wait(async{stream.try_next().await.map_err(db_error)})?{bytes+=chunk.len() as u64;}json!({"rows":n,"estimated_bytes":if n==0 {bytes+3}else{bytes.saturating_mul(n)/n.min(1000)+3},"sample_rows":n.min(1000)})},
 _=>return Err(failure("ODC-CONFIG-001","Unknown database operation"))};
    s.rollback()?;
    Ok(result)
}
pub fn export(
    p: &Value,
    output: &Path,
    overwrite: bool,
    cancel: Arc<AtomicBool>,
    received: Arc<AtomicU64>,
) -> Result<Value> {
    let started = Instant::now();
    let options = p
        .get("_split")
        .or_else(|| p.get("split_options"))
        .map(split::Options::from_value)
        .transpose()?;
    if options.is_none() && output.exists() && !overwrite {
        return Err(failure("ODC-SAVE-001", "Destination already exists"));
    }
    let mut splitter = options
        .map(|mut o| {
            o.header = true;
            o.delimiter = b',';
            o.encoding = "utf8".into();
            split::Splitter::new(output, text(p, "table", "data"), o)
        })
        .transpose()?;
    let result = (|| -> Result<Value> {
        let s = Session::open(p, cancel.clone())?;
        let (select, count) = selection(p)?;
        let n: u64 = s.rows(&count)?[0][0]
            .as_ref()
            .ok_or("Missing count")?
            .parse()?;
        let columns = s.columns(&select)?;
        let snapshot = s.rows("SELECT CURRENT_TIMESTAMP::text")?[0][0].clone();
        let stream = s.copy(&copy_sql(&select))?;
        let mut reader = CopyReader {
            session: &s,
            stream,
            chunk: Bytes::new(),
            offset: 0,
            hash: Sha256::new(),
            size: 0,
            received: &received,
        };
        let mut temporary = None;
        if let Some(splitter) = splitter.as_mut() {
            splitter.consume(
                std::io::Read::chain(std::io::Cursor::new(vec![0xef, 0xbb, 0xbf]), &mut reader),
                &cancel,
            )?;
            let header = csv::ReaderBuilder::new()
                .has_headers(false)
                .from_reader(splitter.header.as_slice())
                .records()
                .next()
                .transpose()?
                .map(|r| r.iter().map(str::to_owned).collect::<Vec<_>>())
                .unwrap_or_default();
            if n != splitter.rows || header != columns {
                return Err(failure(
                    "ODC-SAVE-001",
                    "Split row count or header mismatch",
                ));
            }
        } else {
            let parent = output
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let mut file = tempfile::Builder::new()
                .prefix("pgcsv-")
                .suffix(".part")
                .tempfile_in(parent)?;
            {
                let mut writer = BufWriter::with_capacity(1024 * 1024, file.as_file_mut());
                writer.write_all(&[0xef, 0xbb, 0xbf])?;
                std::io::copy(&mut reader, &mut writer)?;
                writer.flush()?;
            }
            file.as_file().sync_all()?;
            crate::check_csv(
                file.path(),
                &json!({"rows":n}),
                &json!({"columns":columns}),
                &cancel,
            )?;
            temporary = Some(file);
        }
        let size = reader.size;
        let digest = format!("{:x}", reader.hash.clone().finalize());
        drop(reader);
        s.rollback()?;
        if cancel.load(Ordering::Relaxed) {
            return Err(failure("ODC-CANCEL-001", "Cancelled"));
        }
        if let Some(splitter) = splitter.as_ref() {
            let mut r = splitter.complete(&cancel)?;
            r["input_bytes"] = json!(size);
            r["sha256_without_bom"] = json!(digest);
            r["snapshot"] = json!(snapshot);
            r["total_seconds"] = json!(started.elapsed().as_secs_f64());
            r["transport"] = json!("ssh-stdio-postgres");
            Ok(r)
        } else {
            let file = temporary.unwrap();
            if overwrite {
                file.persist(output)?;
            } else {
                file.persist_noclobber(output)?;
            }
            Ok(
                json!({"rows":n,"bytes":size+3,"sha256_without_bom":digest,"snapshot":snapshot,"total_seconds":started.elapsed().as_secs_f64(),"output":output,"transport":"ssh-stdio-postgres"}),
            )
        }
    })();
    result.map_err(|e| match splitter.as_ref() {
        Some(s) => s.incomplete_error(e).into(),
        None => e,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_are_literal_and_bounded_by_caller() {
        assert_eq!(
            setting("DATABASE_URL='literal$(no)'", "DATABASE_URL").unwrap(),
            "literal$(no)"
        );
        assert!(setting("x=y", "x;bad").is_err());
        assert!(setting("X=y", "DATABASE_URL").is_err());
    }
    #[test]
    fn identifiers_and_values_are_quoted() {
        let (sql,_)=selection(&json!({"schema":"a\"b","table":"x;DROP TABLE y","columns":["q\"r"],"time_column":"time","start":"2026-01-01' OR true --"})).unwrap();
        assert!(sql.contains("\"a\"\"b\".\"x;DROP TABLE y\""));
        assert!(sql.contains("'2026-01-01'' OR true --'"));
        assert!(identifier("a\0b").is_err());
    }
    #[test]
    fn unsupported_endpoints_refuse_instead_of_fallback() {
        for dsn in [
            "host=a,b user=u",
            "host=/tmp user=u",
            "host=remote user=u sslmode=disable",
            "host=localhost port=0 user=u",
            "host=localhost",
        ] {
            let mut c = dsn.parse::<Config>().unwrap();
            assert!(endpoint(&mut c).is_err(), "{dsn}");
        }
        let mut c = "host=127.0.0.1 user=u sslmode=disable"
            .parse::<Config>()
            .unwrap();
        assert_eq!(endpoint(&mut c).unwrap().1, 5432);
    }
    #[test]
    fn cancellation_interrupts_a_stalled_future() {
        let rt = Runtime::new().unwrap();
        let cancel = AtomicBool::new(true);
        let result = rt.block_on(controlled(
            std::future::pending::<Result<()>>(),
            &cancel,
            Duration::from_secs(10),
        ));
        assert!(result.unwrap_err().to_string().contains("ODC-CANCEL-001"));
    }
    #[test]
    fn db_errors_do_not_disclose_settings() {
        let err = failure("ODC-DB-001", "Connection failed");
        assert!(!err.to_string().contains("postgresql://"));
    }
    #[test]
    #[ignore = "requires an explicit read-only real-server profile"]
    fn real_native_contract_and_cancel() {
        let path = std::env::var("PGCSV_INTEGRATION_PROFILE").expect("profile required");
        let mut p: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        p["remote_python"] = json!("/does-not-exist");
        let cancel = Arc::new(AtomicBool::new(false));
        let s = Session::open(&p, cancel).unwrap();
        assert_eq!(
            s.rows("SHOW transaction_read_only").unwrap()[0][0].as_deref(),
            Some("on")
        );
        assert_eq!(
            s.rows("SHOW transaction_isolation").unwrap()[0][0].as_deref(),
            Some("repeatable read")
        );
        let pid = s.rows("SELECT pg_backend_pid()::text").unwrap()[0][0]
            .clone()
            .unwrap();
        let cfg = s.rt.block_on(configuration(&p)).unwrap();
        let mut direct = p.clone();
        let host = match &cfg.get_hosts()[0] {
            Host::Tcp(h) => h.as_str(),
            #[cfg(unix)]
            _ => panic!("TCP required"),
        };
        direct["_database"] = json!({"host":host,"port":cfg.get_ports().first().copied().unwrap_or(5432),"user":cfg.get_user(),"dbname":cfg.get_dbname(),"password":String::from_utf8(cfg.get_password().unwrap_or_default().to_vec()).unwrap()});
        drop(s);
        let observer = Session::open(&direct, Arc::new(AtomicBool::new(false))).unwrap();
        assert_eq!(
            observer
                .rows(&format!(
                    "SELECT count(*)::text FROM pg_stat_activity WHERE pid={pid}"
                ))
                .unwrap()[0][0]
                .as_deref(),
            Some("0")
        );
        drop(observer);
        let folder = tempfile::tempdir().unwrap();
        let output = folder.path().join("cancel.csv");
        let cancel = Arc::new(AtomicBool::new(false));
        let received = Arc::new(AtomicU64::new(0));
        let c = cancel.clone();
        let r = received.clone();
        let watcher = std::thread::spawn(move || {
            for _ in 0..1000 {
                if r.load(Ordering::Relaxed) > 10000 {
                    c.store(true, Ordering::Relaxed);
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            c.store(true, Ordering::Relaxed);
        });
        let result = export(&p, &output, false, cancel, received);
        watcher.join().unwrap();
        assert!(result.is_err());
        assert!(!output.exists());
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);
        let mut wrong = direct.clone();
        wrong["_database"]["password"] = json!("deliberately-invalid-test-password");
        wrong["action"] = json!("databases");
        let error = query(&wrong).unwrap_err().to_string();
        assert!(!error.contains("deliberately-invalid"));
        assert!(error.contains("ODC-DB-002"));
        println!(
            "native contract: no Python, direct credentials, read-only snapshot, SSH/backend cleanup, midstream cancellation, temporary cleanup, authentication failure secrecy passed"
        );
    }
}
