fn main() {
    if pgcsv::askpass() {
        return;
    }
    if let Err(error) = pgcsv::cli::run() {
        let (report, status) = pgcsv::cli::error_report(&error.to_string());
        eprintln!("{report}");
        std::process::exit(status)
    }
}
