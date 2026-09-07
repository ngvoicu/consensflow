use app_lib::bridge::stdin_is_pipe;
use app_lib::commands::run_headless;

fn main() {
    if !stdin_is_pipe() {
        return;
    }
    if let Err(error) = run_headless() {
        eprintln!("consensflow-bridge: {error}");
        std::process::exit(1);
    }
}
