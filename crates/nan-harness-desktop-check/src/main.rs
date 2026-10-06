#[tokio::main]
async fn main() {
    let code = match nan_harness_desktop_check::cli::execute().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            2
        }
    };
    std::process::exit(code);
}
