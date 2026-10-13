use std::{fs, process::Command};
fn command(root: &std::path::Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_odc"));
    c.current_dir(root)
        .env("APPDATA", root)
        .env("XDG_CONFIG_HOME", root);
    c
}
#[test]
fn cli_unicode_split_and_no_overwrite() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    fs::write(
        root.join("日本語.csv"),
        "名前,値\n\"改行\nあり\",1\n二,2\n三,3\n",
    )
    .unwrap();
    let result = command(root)
        .args([
            "split",
            "日本語.csv",
            "--split-rows",
            "3",
            "--non-interactive",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8(result.stdout).unwrap().contains("3"));
    let folder = fs::read_dir(root)
        .unwrap()
        .map(|p| p.unwrap().path())
        .find(|p| p.is_dir() && p.file_name().unwrap() != "jp.kazuki.pgcsv")
        .unwrap();
    let mut records = 0;
    let mut files = 0;
    for p in fs::read_dir(folder)
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "csv"))
    {
        let mut reader = csv::Reader::from_path(p).unwrap();
        assert_eq!(&reader.headers().unwrap()[0], "名前");
        let count = reader.records().count();
        assert!(count <= 2);
        records += count;
        files += 1;
    }
    assert_eq!(records, 3);
    assert_eq!(files, 2);
    assert_eq!(
        fs::read_to_string(root.join("日本語.csv")).unwrap(),
        "名前,値\n\"改行\nあり\",1\n二,2\n三,3\n"
    );
}
#[test]
fn utf8_profile_listing_and_error_redirect() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    fs::create_dir(root.join("jp.kazuki.pgcsv")).unwrap();
    fs::write(
        root.join("jp.kazuki.pgcsv/profiles.json"),
        r#"{"日本語設定":{},"second":{}}"#,
    )
    .unwrap();
    let list = command(root).arg("-p").output().unwrap();
    assert!(list.status.success());
    let text = String::from_utf8(list.stdout).unwrap();
    assert!(text.lines().any(|s| s == "日本語設定"));
    let error = command(root).args(["--未知"]).output().unwrap();
    assert!(!error.status.success());
    let error = String::from_utf8(error.stderr).unwrap();
    assert!(error.contains("ODC-CONFIG-001"));
    assert!(!error.contains("--未知"));
    assert!(error.contains("docs/ERRORS.md#odc-config-001"));
    let brief = command(root).output().unwrap();
    let help = command(root).arg("-h").output().unwrap();
    assert!(brief.status.success() && help.status.success());
    assert!(brief.stdout.len() < help.stdout.len());
    for flag in ["-y", "--yes"] {
        let result = command(root)
            .args(["update", "--check", flag])
            .output()
            .unwrap();
        assert!(!result.status.success());
    }
}

#[test]
fn error_report_uses_catalog_and_never_echoes_raw_details() {
    for (detail, expected, status) in [
        ("ODC-DB-002: password=private-test-secret", "ODC-DB-002", 1),
        ("ODC-NOTREAL-001: private-test-secret", "ODC-UNKNOWN-001", 1),
        ("cancel-private-test-secret", "ODC-UNKNOWN-001", 1),
        ("Cancelled", "ODC-CANCEL-001", 130),
    ] {
        let (report, exit) = pgcsv::cli::error_report(detail);
        assert!(report.contains(expected));
        assert!(report.ends_with(&expected.to_lowercase()));
        assert!(!report.contains("private-test-secret"));
        assert_eq!(exit, status);
    }
    let d = tempfile::tempdir().unwrap();
    for args in [
        vec!["--lang", "ja", "--private-test-secret"],
        vec!["--lang=ja", "--private-test-secret"],
    ] {
        let result = command(d.path()).args(args).output().unwrap();
        assert_eq!(result.status.code(), Some(1));
        let text = String::from_utf8(result.stderr).unwrap();
        assert!(text.contains("詳細: https://github.com/ours-labs/data-converter/blob/main/docs/ERRORS.md#odc-config-001"));
        assert!(!text.contains("private-test-secret"));
        assert_eq!(text.lines().count(), 2);
    }
}
