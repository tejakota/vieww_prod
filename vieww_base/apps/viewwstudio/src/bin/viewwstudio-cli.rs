//! `viewwstudio-cli` — the studio's render paths, without the window.
//!
//! Not a second tool: the same compile, the same scaffold, the same export
//! plan the windowed studio uses, behind a command line. See
//! `viewwstudio::cli`'s module doc for what each subcommand shares with the
//! app, and the honest limits of each output.
//!
//! Exit codes are the interface, as `vieww-cli` puts it: 0 for a written
//! artefact, 1 for anything else, with the reason on stderr and the usage
//! beside any argument error.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = match viewwstudio::cli::parse(&args) {
        Ok(command) => command,
        Err(usage) if usage.is_empty() => {
            print!("{}", viewwstudio::cli::usage());
            return ExitCode::from(64); // EX_USAGE: the argument list was wrong
        }
        Err(error) => {
            eprintln!("viewwstudio-cli: {error}\n\n{}", viewwstudio::cli::usage());
            return ExitCode::from(64);
        }
    };
    let result = match &command {
        viewwstudio::cli::Command::New { name, rust } => {
            let parent = std::env::current_dir()
                .map_err(|error| format!("cannot tell where to put {name} — {error}"));
            match parent {
                Ok(parent) => viewwstudio::cli::run_new(&parent, name, *rust),
                Err(error) => Err(error),
            }
        }
        viewwstudio::cli::Command::Render {
            input,
            out,
            size,
            dark,
        } => viewwstudio::cli::run_render(input, out.as_deref(), *size, *dark),
        viewwstudio::cli::Command::Html {
            input,
            out,
            size,
            dark,
        } => viewwstudio::cli::run_html(input, out.as_deref(), *size, *dark),
        viewwstudio::cli::Command::Web { project } => viewwstudio::cli::run_web(project),
        viewwstudio::cli::Command::Screens { input } => viewwstudio::cli::run_screens(input),
    };
    match result {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("viewwstudio-cli: {error}");
            ExitCode::FAILURE
        }
    }
}
