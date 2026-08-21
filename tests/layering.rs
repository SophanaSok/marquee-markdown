//! Enforces the architectural boundaries that keep the pieces reusable and the
//! draw path pure. Each rule here has a reason recorded with it; if one fails,
//! the fix is almost always to move the code rather than to relax the rule.

use std::path::{Path, PathBuf};

/// One layering rule: nothing under `dir` may name any of `forbidden`.
struct Layer {
    dir: &'static str,
    forbidden: &'static [&'static str],
    reason: &'static str,
}

const LAYERS: &[Layer] = &[
    Layer {
        dir: "src/render",
        forbidden: &["crate::app", "crate::ui", "crate::browser", "crate::doc"],
        reason: "the render engine is the reusable core; keeping it free of the \
                 shell is what makes extracting it into its own crate mechanical \
                 rather than archaeological",
    },
    Layer {
        dir: "src/source",
        forbidden: &["crate::app", "crate::ui", "crate::browser"],
        reason: "resolving an argument into a document is upstream of anything \
                 on screen; keeping it there is what lets classification and \
                 fetching be tested with no filesystem and no network",
    },
    Layer {
        dir: "src/browser",
        forbidden: &["crate::app", "crate::ui"],
        reason: "the browser models a directory of files, not a screen; keeping \
                 it below the shell is what lets the walk, the filter and the \
                 selection be tested without a terminal",
    },
    Layer {
        dir: "src/doc",
        forbidden: &["crate::app", "crate::ui", "crate::browser"],
        reason: "document state is modelled without reference to a terminal, \
                 which is what lets the layout cache and the scroll arithmetic \
                 be tested without drawing anything",
    },
];

#[test]
fn each_layer_stays_below_the_one_above_it() {
    for layer in LAYERS {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(layer.dir);
        let mut offenders = Vec::new();
        for path in rust_files(&dir) {
            let source = std::fs::read_to_string(&path).expect("readable source");
            for (lineno, line) in source.lines().enumerate() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for needle in layer.forbidden {
                    if line.contains(needle) {
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
        assert!(
            offenders.is_empty(),
            "{} must not reference {:?} — {}:\n{}",
            layer.dir,
            layer.forbidden,
            layer.reason,
            offenders.join("\n")
        );
    }
}

#[test]
fn drawing_is_never_handed_the_means_to_mutate() {
    // The house rule is that widgets derive from state and never change it.
    // A `&mut App` in the draw path is how that erodes: pane sizes get
    // computed during render, then the frame stops being reproducible and the
    // headless key-sequence tests stop meaning anything.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui");
    let mut offenders = Vec::new();
    for path in rust_files(&dir) {
        let source = std::fs::read_to_string(&path).expect("readable source");
        for (lineno, line) in source.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("pub fn") && trimmed.contains("&mut App") {
                offenders.push(format!("{}:{}: {}", path.display(), lineno + 1, trimmed));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "widgets in src/ui must take &App, not &mut App:\n{}",
        offenders.join("\n")
    );
}

/// Exactly one module may read the terminal.
///
/// Crossterm's reader is process-global, and it forbids being driven from more
/// than one thread. The whole handoff to an editor rests on that: input can
/// only be stood down for another program if there is a single, known place
/// that reads it. A second `event::read` somewhere else would not fail a test
/// or a build — it would quietly go back to splitting keystrokes with whatever
/// the reader launched.
#[test]
fn only_one_module_reads_the_terminal() {
    const READER: &str = "src/app/event.rs";
    let mut offenders = Vec::new();
    for path in rust_files(Path::new("src")) {
        if path == Path::new(READER) {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("source is readable");
        for (lineno, line) in source.lines().enumerate() {
            if line.contains("event::read(") || line.contains("event::poll(") {
                offenders.push(format!(
                    "{}:{}: {}",
                    path.display(),
                    lineno + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "only {READER} may read the terminal; route it through there instead:\n{}",
        offenders.join("\n")
    );
}

/// Only one module may ask the terminal a question.
///
/// A question is a write followed by a read of the reply, and a reply that
/// reaches the wrong reader is gone. `src/util/osc.rs` puts its questions on
/// `/dev/tty` exactly once, before the screen is taken and before the event
/// thread exists; a second place doing the same thing would race that thread
/// for the answer and swallow a keystroke to get it. This is the same bug
/// `src/app/gate.rs` exists to prevent, approached from the other side.
#[test]
fn only_one_module_asks_the_terminal_a_question() {
    const ASKER: &str = "src/util/osc.rs";
    let mut offenders = Vec::new();
    for path in rust_files(Path::new("src")) {
        if path == Path::new(ASKER) {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("source is readable");
        for (lineno, line) in source.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if line.contains("/dev/tty") {
                offenders.push(format!(
                    "{}:{}: {}",
                    path.display(),
                    lineno + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "only {ASKER} may ask the terminal about itself; route it through there \
         instead:\n{}",
        offenders.join("\n")
    );
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("directory exists") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    assert!(!files.is_empty(), "no sources under {}", dir.display());
    files
}
