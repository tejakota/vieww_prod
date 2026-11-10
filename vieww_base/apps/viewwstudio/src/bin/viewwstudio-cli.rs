//! `viewwstudio-cli` — the studio, without the window.
//!
//! Not a second tool: the same compile, the same scaffold, the same export
//! plan, the same devices, the same preview child the windowed studio uses,
//! behind a command line. See `viewwstudio::cli`'s module doc for the
//! capability table and what each subcommand shares with the app.
//!
//! Exit codes are the interface, as `vieww-cli` puts it: 0 for a written
//! artefact, 1 for anything else, with the reason on stderr and the usage
//! beside any argument error. The one exception is `preview`, which serves
//! until it is killed — Ctrl-C is its success path.

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
    use viewwstudio::cli::Command;
    let result = match &command {
        Command::New { name, rust } => {
            let parent = std::env::current_dir()
                .map_err(|error| format!("cannot tell where to put {name} — {error}"));
            match parent {
                Ok(parent) => viewwstudio::cli::run_new(&parent, name, *rust),
                Err(error) => Err(error),
            }
        }
        Command::Screen { name } => viewwstudio::cli::run_screen(name),
        Command::Widget { name } => viewwstudio::cli::run_widget(name),
        Command::Render {
            input,
            out,
            viewport,
            dark,
            screen,
            all_screens,
            present,
        } => viewwstudio::cli::run_render(
            input,
            out.as_deref(),
            viewport,
            *dark,
            screen.as_deref(),
            *all_screens,
            present,
        ),
        Command::Html {
            input,
            out,
            viewport,
            dark,
            screen,
        } => viewwstudio::cli::run_html(input, out.as_deref(), viewport, *dark, screen.as_deref()),
        Command::Animate {
            input,
            out,
            viewport,
            dark,
            screen,
            fps,
            duration,
            frames_dir,
            present,
        } => viewwstudio::cli::run_animate(
            input,
            out.as_deref(),
            viewport,
            *dark,
            screen.as_deref(),
            *fps,
            *duration,
            frames_dir.as_deref(),
            present,
        ),
        Command::Preview {
            input,
            device,
            dark,
            screen,
            port,
            scale,
            open,
            share,
        } => viewwstudio::cli::run_preview(
            input.as_deref(),
            *device,
            *dark,
            screen.as_deref(),
            *port,
            *scale,
            *open,
            *share,
        ),
        Command::Export {
            project,
            format,
            all,
            list,
        } => viewwstudio::cli::run_export(project.as_deref(), *format, *all, *list),
        Command::Web { project } => viewwstudio::cli::run_web(project),
        Command::Test {
            input,
            viewport,
            dark,
            goldens,
            update,
        } => viewwstudio::cli::run_test(
            input.as_deref(),
            viewport,
            *dark,
            goldens.as_deref(),
            *update,
        ),
        Command::Screens { input } => viewwstudio::cli::run_screens(input),
        Command::Devices => viewwstudio::cli::run_devices(),
        Command::Assets { project } => viewwstudio::cli::run_assets(project.as_deref()),
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
