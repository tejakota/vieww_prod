//! ST-17: `Copy diagnostics` carries what a supporter needs and never the
//! user's code.

use std::path::PathBuf;
use std::rc::Rc;

use vieww_element::Runtime;
use viewwstudio::command::Command;
use viewwstudio::state::{Diagnostic, Severity, Studio};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "viewwstudio-diagnostics-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp dir");
        Self(std::fs::canonicalize(&path).unwrap_or(path))
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const SECRET: &str = "sk-live-4f9a1c0ffee0secret";

#[test]
fn the_report_has_the_fields_and_none_of_the_code() {
    let dir = Dir::new("report");
    std::fs::write(
        dir.0.join("main.rs"),
        format!("fn main() {{\n    let key = \"{SECRET}\";\n}}\n"),
    )
    .unwrap();
    let runtime = Runtime::new();
    let studio = Studio::new(&runtime);
    studio.open_workspace(&dir.0);
    studio.open_path(dir.0.join("main.rs"));
    // An unsaved edit, also carrying the secret.
    let mut value = studio.active().expect("the file is open").value;
    value.text.push_str(&format!("// {SECRET}\n"));
    studio.edit(value);

    studio.diagnostics.set(Rc::new(vec![Diagnostic {
        file: "main.rs".into(),
        severity: Severity::Warning,
        code: "unused_variables".into(),
        message: "unused variable: `key`".into(),
        help: Some("if this is intentional, prefix it with an underscore".into()),
        line: 2,
        column: 9,
        end_line: 2,
        end_column: 12,
    }]));

    let report = viewwstudio::diagnostics::report(&studio);
    for field in [
        "Version",
        "Build",
        "Target",
        "Toolchains",
        "Desktop",
        "Problems (1 total",
        "main.rs:2:9 warning [unused_variables] unused variable: `key`",
        "Workspace",
        "Last render",
        "main.rs (unsaved)",
    ] {
        assert!(report.contains(field), "missing {field:?} in:\n{report}");
    }
    assert!(
        !report.contains(SECRET),
        "buffer text leaked into the report:\n{report}"
    );

    // The command is in the palette and runs headless.
    assert!(Command::ALL.contains(&Command::CopyDiagnostics));
    assert!(studio.can_run(Command::CopyDiagnostics));
    studio.run(Command::CopyDiagnostics);
}
