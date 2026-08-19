//! Enforces the architectural boundary that keeps the renderer reusable: the
//! render engine must never reference the application shell. If this test
//! fails, the offending `use` belongs in a shell module, or the shared type
//! belongs in `render`.

use std::path::Path;

const FORBIDDEN: &[&str] = &["crate::app", "crate::ui", "crate::browser", "crate::doc"];

#[test]
fn render_engine_does_not_depend_on_the_app_shell() {
    let render_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/render");
    let mut offenders = Vec::new();
    visit(&render_dir, &mut offenders);
    assert!(
        offenders.is_empty(),
        "src/render must not reference the app shell:\n{}",
        offenders.join("\n")
    );
}

fn visit(dir: &Path, offenders: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("render dir exists") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            visit(&path, offenders);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let source = std::fs::read_to_string(&path).expect("readable source");
            for needle in FORBIDDEN {
                for (lineno, line) in source.lines().enumerate() {
                    if line.contains(needle) && !line.trim_start().starts_with("//") {
                        offenders.push(format!(
                            "{}:{}: {}",
                            path.display(),
                            lineno + 1,
                            line.trim()
                        ));
                    }
                }
            }
        }
    }
}
