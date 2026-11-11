use std::io::Write;
use std::process::ExitCode;

use clap::Parser;
use coffer_app::cli::{self, Cli};
use coffer_core::paths;

fn main() -> ExitCode {
    let args = Cli::parse();
    match args.command {
        // The headless commands never start GPUI.
        Some(command) => run_command(command),
        None => {
            run_gui(args.showcase);
            ExitCode::SUCCESS
        }
    }
}

/// Runs a CLI command against the database in the data dir
/// (`COFFER_DATA_DIR` when set). Output on stdout, errors on stderr.
fn run_command(command: cli::Command) -> ExitCode {
    let result = paths::database_path()
        .map_err(anyhow::Error::from)
        .and_then(|database| {
            let mut stdout = std::io::stdout().lock();
            let result = cli::run(command, &database, &mut stdout);
            stdout.flush()?;
            result
        });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Opens the app, or with `--showcase` the component showcase (D2.5).
fn run_gui(showcase: bool) {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            // Must be called before using any GPUI Kit components.
            gpui_kit::init(cx);
            coffer_app::theme::install(cx).expect("Failed to install the theme");

            if showcase {
                coffer_app::showcase::init(cx);
                coffer_app::showcase::open(cx).expect("Failed to open the showcase");
            } else {
                // The Transactions screen on fixture data (D5) until the
                // app shell arrives (M2).
                coffer_app::transactions::init(cx);
                coffer_app::transactions::open(cx).expect("Failed to open the window");
            }
        });
}
