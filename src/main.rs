fn main() {
    if pgcsv::askpass() {
        return;
    }
    if let Err(error) = pgcsv::run_cli() {
        eprintln!(
            "{}",
            serde_json::json!({"status":"failed","detail":error.to_string()})
        );
        std::process::exit(1);
    }
}
