//! Serves LSP on stdin / stdout (`--stdio`, what editors pass, is the only way it talks).

fn main() -> std::process::ExitCode {
    if std::env::args().any(|a| a == "--version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return std::process::ExitCode::SUCCESS;
    }
    match language_server::serve(std::io::stdin(), std::io::stdout().lock()) {
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) => std::process::ExitCode::FAILURE,
        Err(e) => {
            eprintln!("language-server: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
