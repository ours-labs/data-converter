//! Strict, byte-preserving CSV record splitting. Never rewrites field quoting.
use crate::Result;
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const BOM: &[u8] = b"\xef\xbb\xbf";
const MAX_RECORD: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Options {
    pub limit: u64,
    pub header: bool,
    pub count_header: bool,
    pub delimiter: u8,
    pub encoding: String,
}

impl Options {
    pub fn from_value(v: &Value) -> Result<Self> {
        let options = Self {
            limit: v["limit"]
                .as_u64()
                .ok_or("Enter a positive whole-number row limit")?,
            header: v["header"].as_bool().ok_or("header must be boolean")?,
            count_header: v["count_header"]
                .as_bool()
                .ok_or("count_header must be boolean")?,
            delimiter: match v["delimiter"].as_str() {
                Some(",") => b',',
                Some("\t") => b'\t',
                Some(";") => b';',
                _ => return Err("Select comma, tab or semicolon".into()),
            },
            encoding: v["encoding"]
                .as_str()
                .ok_or("Select an encoding")?
                .to_owned(),
        };
        options.validate()?;
        Ok(options)
    }
    fn validate(&self) -> Result<()> {
        if self.limit == 0 || (self.header && self.count_header && self.limit < 2) {
            return Err(
                "Row limit must be at least 2 when counting the header, otherwise at least 1"
                    .into(),
            );
        }
        if !b",\t;".contains(&self.delimiter)
            || !["auto", "utf8", "cp932"].contains(&self.encoding.as_str())
        {
            return Err("Unsupported CSV format".into());
        }
        Ok(())
    }
    fn capacity(&self) -> u64 {
        self.limit - u64::from(self.header && self.count_header)
    }
}

// CSV grammars must be checked explicitly: csv::Reader deliberately accepts
// malformed quoting. Buffer at most one bounded logical record, not the file.
fn record<R: BufRead>(
    r: &mut R,
    delimiter: u8,
    cancel: &AtomicBool,
) -> Result<Option<(Vec<u8>, usize)>> {
    let mut raw = Vec::new();
    let (mut state, mut fields) = (0u8, 1usize); // start, unquoted, quoted, after quote
    loop {
        if raw.len() % 8192 == 0 && cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let buf = r.fill_buf()?;
        if buf.is_empty() {
            if state == 2 {
                return Err("Unclosed quoted field".into());
            }
            return Ok((!raw.is_empty()).then_some((raw, fields)));
        }
        let byte = buf[0];
        r.consume(1);
        raw.push(byte);
        if raw.len() > MAX_RECORD {
            return Err("CSV record exceeds 64 MiB".into());
        }
        if state != 2 && matches!(byte, b'\r' | b'\n') {
            if byte == b'\r' && r.fill_buf()?.first() == Some(&b'\n') {
                raw.push(b'\n');
                r.consume(1);
            }
            if raw.len() > MAX_RECORD {
                return Err("CSV record exceeds 64 MiB".into());
            }
            return Ok(Some((raw, fields)));
        }
        state = match (state, byte) {
            (0, b'"') => 2,
            (0 | 1 | 3, b) if b == delimiter => {
                fields += 1;
                0
            }
            (1, b'"') => return Err("Quote inside an unquoted field".into()),
            (0 | 1, _) => 1,
            (2, b'"') => 3,
            (2, _) => 2,
            (3, b'"') => 2,
            (3, _) => return Err("Unexpected character after closing quote".into()),
            _ => unreachable!(),
        };
    }
}

pub struct Splitter {
    parent: PathBuf,
    stem: String,
    options: Options,
    folder: Option<PathBuf>,
    writer: Option<BufWriter<File>>,
    rows_in_file: u64,
    pub rows: u64,
    pub files: u64,
    bytes: u64,
    pub header: Vec<u8>,
    bom: bool,
}

impl Splitter {
    pub fn new(parent: &Path, stem: &str, options: Options) -> Result<Self> {
        options.validate()?;
        if !parent.is_dir() {
            return Err("Choose an existing destination folder".into());
        }
        let stem: String = stem
            .chars()
            .map(|c| {
                if c.is_control() || "<>:\"/\\|?*".contains(c) {
                    '_'
                } else {
                    c
                }
            })
            .take(80)
            .collect();
        Ok(Self {
            parent: parent.to_owned(),
            stem: if stem.is_empty() { "data".into() } else { stem },
            options,
            folder: None,
            writer: None,
            rows_in_file: 0,
            rows: 0,
            files: 0,
            bytes: 0,
            header: vec![],
            bom: false,
        })
    }
    fn flush(&mut self) -> Result<()> {
        if let Some(mut w) = self.writer.take() {
            w.flush()?;
            w.get_ref().sync_all()?;
        }
        Ok(())
    }
    fn next_file(&mut self) -> Result<()> {
        self.flush()?;
        if self.folder.is_none() {
            let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
            for suffix in 1u64.. {
                let dir = self
                    .parent
                    .join(format!("{}_split_{stamp}_{suffix:03}", self.stem));
                match fs::create_dir(&dir) {
                    Ok(()) => {
                        self.folder = Some(dir);
                        break;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(e.into()),
                }
            }
            fs::write(
                self.folder.as_ref().unwrap().join("INCOMPLETE.txt"),
                b"Incomplete CSV output. Do not treat these files as a completed export.\n",
            )?;
        }
        self.files += 1;
        let path = self
            .folder
            .as_ref()
            .unwrap()
            .join(format!("{}_{:03}.csv", self.stem, self.files));
        let mut writer = BufWriter::with_capacity(
            1024 * 1024,
            OpenOptions::new().write(true).create_new(true).open(path)?,
        );
        if self.bom {
            writer.write_all(BOM)?;
            self.bytes += 3;
        }
        if self.options.header {
            writer.write_all(&self.header)?;
            self.bytes += self.header.len() as u64;
        }
        self.writer = Some(writer);
        self.rows_in_file = 0;
        Ok(())
    }
    pub fn consume<R: Read>(&mut self, mut input: R, cancel: &AtomicBool) -> Result<()> {
        let mut prefix = vec![];
        for _ in 0..3 {
            let mut b = [0];
            if input.read(&mut b)? == 0 {
                break;
            }
            prefix.push(b[0]);
        }
        self.bom = prefix == BOM;
        if self.bom && self.options.encoding == "cp932" {
            return Err("UTF-8 BOM conflicts with CP932 selection".into());
        }
        if self.bom {
            prefix.clear();
        }
        let mut reader =
            BufReader::with_capacity(1024 * 1024, std::io::Cursor::new(prefix).chain(input));
        let mut width = None;
        let mut number = 0u64;
        loop {
            number += 1;
            let row = record(&mut reader, self.options.delimiter, cancel)
                .map_err(|e| format!("CSV record {number}: {e}"))?;
            let Some((raw, fields)) = row else {
                break;
            };
            if self.options.encoding == "cp932" {
                if encoding_rs::SHIFT_JIS
                    .decode_without_bom_handling_and_without_replacement(&raw)
                    .is_none()
                {
                    return Err(format!("CSV record {number}: invalid CP932 bytes").into());
                }
            } else if std::str::from_utf8(&raw).is_err() {
                return Err(format!("CSV record {number}: invalid UTF-8. Select CP932 if appropriate; encoding is not guessed.").into());
            }
            if *width.get_or_insert(fields) != fields {
                return Err(format!(
                    "CSV record {number}: expected {} fields, found {fields}",
                    width.unwrap()
                )
                .into());
            }
            if number == 1 && self.options.header {
                self.header = raw;
                continue;
            }
            if self.writer.is_none() || self.rows_in_file == self.options.capacity() {
                self.next_file()?;
            }
            self.writer.as_mut().unwrap().write_all(&raw)?;
            self.bytes += raw.len() as u64;
            self.rows += 1;
            self.rows_in_file += 1;
        }
        self.flush()?;
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        Ok(())
    }
    pub fn incomplete_error(&self, error: impl std::fmt::Display) -> String {
        match &self.folder {
            Some(p) => format!("{error}\nIncomplete folder: {}", p.display()),
            None => error.to_string(),
        }
    }
    pub fn complete(&self, cancel: &AtomicBool) -> Result<Value> {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        if let Some(p) = &self.folder {
            fs::remove_file(p.join("INCOMPLETE.txt"))?;
        }
        Ok(
            json!({"rows":self.rows,"files":self.files,"bytes":self.bytes,"output":self.folder,"empty":self.rows==0,"encoding":if self.bom { "utf8-bom" } else if self.options.encoding=="cp932" {"cp932"} else {"utf8"}}),
        )
    }
}

/// Read-only inspection using the same strict logical-record parser as splitting.
/// Returns total records; header interpretation and split limits are UI choices.
pub fn inspect_file(input: &Path, options: &Options, cancel: &AtomicBool) -> Result<Value> {
    options.validate()?;
    let file = File::open(input)?;
    let before = file.metadata()?;
    if !before.is_file() {
        return Err("Choose a regular CSV file".into());
    }
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let bom = reader.fill_buf()?.starts_with(BOM);
    if bom && options.encoding == "cp932" {
        return Err("UTF-8 BOM conflicts with CP932 selection".into());
    }
    if bom {
        reader.consume(3);
    }
    let mut records = 0u64;
    let mut width = None;
    while let Some((raw, fields)) = record(&mut reader, options.delimiter, cancel)
        .map_err(|e| format!("CSV record {}: {e}", records + 1))?
    {
        records += 1;
        let valid = if options.encoding == "cp932" {
            encoding_rs::SHIFT_JIS
                .decode_without_bom_handling_and_without_replacement(&raw)
                .is_some()
        } else {
            std::str::from_utf8(&raw).is_ok()
        };
        if !valid {
            return Err(format!(
                "CSV record {records}: invalid encoding; check the selected encoding"
            )
            .into());
        }
        if *width.get_or_insert(fields) != fields {
            return Err(format!("CSV record {records}: inconsistent field count").into());
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Cancelled".into());
    }
    let after = fs::metadata(input)?;
    if before.len() != after.len() || before.modified()? != after.modified()? {
        return Err("File changed during inspection; scan again".into());
    }
    Ok(json!({"records":records,"bytes":before.len()}))
}

pub fn split_file(
    input: &Path,
    parent: &Path,
    options: Options,
    cancel: &AtomicBool,
    received: &AtomicU64,
) -> Result<Value> {
    let started = Instant::now();
    let file = File::open(input)?;
    if !file.metadata()?.is_file() {
        return Err("Choose a regular CSV file".into());
    }
    let mut splitter = Splitter::new(
        parent,
        input.file_stem().and_then(|s| s.to_str()).unwrap_or("data"),
        options,
    )?;
    struct Progress<'a> {
        file: File,
        received: &'a AtomicU64,
        cancel: &'a AtomicBool,
    }
    impl Read for Progress<'_> {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(std::io::Error::other("Cancelled"));
            }
            let n = self.file.read(b)?;
            self.received.fetch_add(n as u64, Ordering::Relaxed);
            Ok(n)
        }
    }
    let result = (|| {
        splitter.consume(
            Progress {
                file,
                received,
                cancel,
            },
            cancel,
        )?;
        let mut result = splitter.complete(cancel)?;
        result["input_bytes"] = json!(received.load(Ordering::Relaxed));
        result["total_seconds"] = json!(started.elapsed().as_secs_f64());
        Ok(result)
    })();
    result.map_err(|e: Box<dyn std::error::Error>| splitter.incomplete_error(e).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options(header: bool, count: bool, delimiter: u8, encoding: &str) -> Options {
        Options {
            limit: 3,
            header,
            count_header: count,
            delimiter,
            encoding: encoding.into(),
        }
    }
    fn run(raw: &[u8], o: Options) -> (tempfile::TempDir, Value, Vec<Vec<u8>>) {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("日本語.csv");
        fs::write(&src, raw).unwrap();
        let r = split_file(
            &src,
            dir.path(),
            o,
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap();
        assert_eq!(fs::read(src).unwrap(), raw);
        let files = if let Some(folder) = r["output"].as_str() {
            assert!(!Path::new(folder).join("INCOMPLETE.txt").exists());
            let mut paths = fs::read_dir(folder)
                .unwrap()
                .map(|x| x.unwrap().path())
                .collect::<Vec<_>>();
            paths.sort();
            paths.iter().map(|p| fs::read(p).unwrap()).collect()
        } else {
            vec![]
        };
        (dir, r, files)
    }
    #[test]
    fn inspection_matches_split_records_and_never_writes() {
        for (raw, encoding) in [
            (b"\xef\xbb\xbfh,v\r\n\"a\r\nb\",1\r\nz,2".as_slice(), "auto"),
            (b"h,v\n\x82\xa0,1\n\x82\xa2,2\n".as_slice(), "cp932"),
            (b"".as_slice(), "utf8"),
            (b"h,v".as_slice(), "utf8"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("日本語.csv");
            fs::write(&path, raw).unwrap();
            let o = options(true, true, b',', encoding);
            let info = inspect_file(&path, &o, &AtomicBool::new(false)).unwrap();
            assert_eq!(info["bytes"], raw.len());
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
            assert_eq!(fs::read(&path).unwrap(), raw);
            let split = split_file(
                &path,
                dir.path(),
                o,
                &AtomicBool::new(false),
                &AtomicU64::new(0),
            )
            .unwrap();
            assert_eq!(
                info["records"].as_u64().unwrap().saturating_sub(1),
                split["rows"].as_u64().unwrap()
            );
            assert!(
                inspect_file(
                    &path,
                    &options(false, false, b',', encoding),
                    &AtomicBool::new(true)
                )
                .unwrap_err()
                .to_string()
                .contains("Cancelled")
            );
        }
        for raw in [b"h,v\n\"bad".as_slice(), b"h,v\nx\n", b"h,v\n\xff,1\n"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("in.csv");
            fs::write(&path, raw).unwrap();
            assert!(
                inspect_file(
                    &path,
                    &options(true, true, b',', "auto"),
                    &AtomicBool::new(false)
                )
                .is_err()
            );
        }
    }
    #[test]
    #[ignore = "explicit read-only inspection of an existing local export"]
    fn inspection_real_export() {
        let path = std::env::var("PGCSV_INFO_FILE").expect("PGCSV_INFO_FILE");
        let result = inspect_file(
            Path::new(&path),
            &options(true, true, b',', "auto"),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(result["records"], 647463);
        assert_eq!(result["bytes"], 169729582);
        println!("{result}");
    }
    #[test]
    fn matrix_preserves_bom_quotes_multiline_null_and_cp932() {
        for delimiter in *b",\t;" {
            for encoding in ["utf8", "cp932"] {
                for header in [false, true] {
                    for count in [false, true] {
                        let sep = char::from(delimiter);
                        let head = format!("名前{sep}値\r\n");
                        let rows = [
                            format!("日本語{sep}\r\n"),
                            format!("\"改行\r\n値\"{sep}\"\"\r\n"),
                            format!("\"a\"\"b\"{sep}3\r\n"),
                            format!("終{sep}4"),
                        ];
                        let encode = |s: &str| {
                            if encoding == "cp932" {
                                encoding_rs::SHIFT_JIS.encode(s).0.into_owned()
                            } else {
                                s.as_bytes().to_vec()
                            }
                        };
                        let mut input = if encoding == "utf8" {
                            BOM.to_vec()
                        } else {
                            vec![]
                        };
                        if header {
                            input.extend(encode(&head));
                        }
                        for row in &rows {
                            input.extend(encode(row));
                        }
                        let (_dir, result, files) =
                            run(&input, options(header, count, delimiter, encoding));
                        assert_eq!(result["rows"], 4);
                        let capacity = if header && count { 2 } else { 3 };
                        assert_eq!(files.len(), 2);
                        for (i, file) in files.iter().enumerate() {
                            let mut expected = if encoding == "utf8" {
                                BOM.to_vec()
                            } else {
                                vec![]
                            };
                            if header {
                                expected.extend(encode(&head));
                            }
                            for row in rows.iter().skip(i * capacity).take(capacity) {
                                expected.extend(encode(row));
                            }
                            assert_eq!(file, &expected);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn empty_header_only_exact_boundary_and_unique_folder() {
        for raw in [b"".as_slice(), BOM, b"head", b"head\n"] {
            let (_d, r, files) = run(raw, options(true, true, b',', "auto"));
            assert_eq!(r["rows"], 0);
            assert!(files.is_empty());
            assert!(r["output"].is_null());
        }
        let (_d, r, files) = run(b"h\na\nb\n", options(true, true, b',', "auto"));
        assert_eq!(r["files"], 1);
        assert_eq!(files[0], b"h\na\nb\n");
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("input.csv");
        fs::write(&src, b"h\nx\n").unwrap();
        let a = split_file(
            &src,
            dir.path(),
            options(true, true, b',', "auto"),
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap();
        let b = split_file(
            &src,
            dir.path(),
            options(true, true, b',', "auto"),
            &AtomicBool::new(false),
            &AtomicU64::new(0),
        )
        .unwrap();
        assert_ne!(a["output"], b["output"]);
    }
    #[test]
    fn malformed_and_encoding_errors_leave_marker_and_record_number() {
        for tail in [
            b"a\"b,c\n".as_slice(),
            b"\"unclosed,c",
            b"\"a\"bad,c\n",
            b"one\n",
            b"\xff,c\n",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("in.csv");
            let raw = [b"h,v\na,1\n".as_slice(), tail].concat();
            fs::write(&src, &raw).unwrap();
            let error = split_file(
                &src,
                dir.path(),
                options(true, true, b',', "auto"),
                &AtomicBool::new(false),
                &AtomicU64::new(0),
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("record 3"), "{error}");
            assert!(error.contains("Incomplete folder:"));
            assert_eq!(fs::read(src).unwrap(), raw);
            let folder = fs::read_dir(dir.path())
                .unwrap()
                .map(|x| x.unwrap().path())
                .find(|p| p.is_dir())
                .unwrap();
            assert!(folder.join("INCOMPLETE.txt").exists());
        }
    }
    #[test]
    fn output_collision_is_not_overwritten_and_failure_stays_incomplete() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Splitter::new(dir.path(), "data", options(true, true, b',', "auto")).unwrap();
        s.consume(b"h\na\n".as_slice(), &AtomicBool::new(false))
            .unwrap();
        let collision = s.folder.as_ref().unwrap().join("data_002.csv");
        fs::write(&collision, b"existing").unwrap();
        assert!(s.next_file().is_err());
        assert_eq!(fs::read(&collision).unwrap(), b"existing");
        assert!(s.folder.as_ref().unwrap().join("INCOMPLETE.txt").exists());
        let (_dir, r, files) = run(b"a,b\r1,2\r3,4\r", options(true, true, b',', "utf8"));
        assert_eq!(r["rows"], 2);
        assert_eq!(files[0], b"a,b\r1,2\r3,4\r");
    }
    #[test]
    fn invalid_limits_cancel_and_read_failure_cannot_complete() {
        for limit in [json!(0), json!(1), json!(-1), json!(1.5)] {
            assert!(Options::from_value(&json!({"limit":limit,"header":true,"count_header":true,"delimiter":",","encoding":"auto"})).is_err());
        }
        let dir = tempfile::tempdir().unwrap();
        let mut s = Splitter::new(dir.path(), "data", options(true, true, b',', "auto")).unwrap();
        assert!(
            s.consume(b"h\na\n".as_slice(), &AtomicBool::new(true))
                .is_err()
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        s.consume(b"h\na\n".as_slice(), &AtomicBool::new(false))
            .unwrap();
        assert!(s.complete(&AtomicBool::new(true)).is_err());
        assert!(s.folder.as_ref().unwrap().join("INCOMPLETE.txt").exists());
        struct Broken(std::io::Cursor<Vec<u8>>);
        impl Read for Broken {
            fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
                let n = self.0.read(b)?;
                if n == 0 {
                    Err(std::io::Error::other("simulated read failure"))
                } else {
                    Ok(n)
                }
            }
        }
        let mut s = Splitter::new(dir.path(), "broken", options(true, true, b',', "auto")).unwrap();
        assert!(
            s.consume(
                Broken(std::io::Cursor::new(b"h\na\n".to_vec())),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert!(s.folder.as_ref().unwrap().join("INCOMPLETE.txt").exists());
    }
}
