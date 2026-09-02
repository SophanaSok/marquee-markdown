//! Configuration: a file, the environment, and the command line, resolved into
//! one set of settings.
//!
//! Three rules hold this together:
//!
//! - Precedence is defined once, in [`layer::Layer::over`]. Not per field, and
//!   not at the call site.
//! - Anything that cannot be understood is a warning, not a failure. A file
//!   written for a newer version must not brick an older binary.
//! - Every source is a pure function of its input, so the whole of it is
//!   tested without a filesystem or an environment.

pub mod keys;
pub mod layer;
pub mod schema;
pub mod write;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::app::keymap::{Keymap, Mode};
pub use layer::Layer;
pub use schema::File;

use crate::render::HtmlMode;
use crate::theme::ThemeVariant;

/// The environment variable naming a configuration file.
pub const CONFIG_ENV: &str = "MARQUEE_CONFIG";

/// The settings the program runs with.
///
/// Adding a setting is routine here, and adding a public field to a struct
/// anyone can write as a literal is a breaking change — which made every
/// new setting cost a release. `non_exhaustive` buys that back: outside
/// this crate these are built from [`Default`] and then assigned to, so a
/// field arriving later is not an API break.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Config {
    /// Theme name or path.
    pub style: String,
    /// Content width; `Some(0)` disables wrapping.
    pub width: Option<u16>,
    /// Line numbers in code blocks.
    pub line_numbers: bool,
    /// Mouse wheel scrolling.
    pub mouse: bool,
    /// List hidden and ignored files when browsing.
    pub all: bool,
    /// Keep the line breaks the author typed.
    pub preserve_new_lines: bool,
    /// Check crates.io for a newer release, and say so on the way out.
    pub update_check: bool,
    /// Let `--style system` ask the terminal what colors it is using.
    pub terminal_query: bool,
    /// Paths whose change means the terminal may have been retinted, and
    /// `--style system` should ask it again.
    pub theme_watch: Vec<PathBuf>,
    /// Start with the contents pane showing.
    pub contents: bool,
    /// Start with the hint line showing above the status bar.
    pub hints: bool,
    /// What to do with raw HTML.
    pub html: HtmlMode,
    /// Key bindings, defaults with the file's changes laid over them.
    pub keymap: Keymap,
    /// Where the settings were read from, if a file was found.
    pub path: Option<PathBuf>,
    /// Anything in the configuration that could not be used. Shown to the
    /// reader rather than swallowed: a setting that silently does nothing is
    /// worse than one that says why.
    pub warnings: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self::from_parts(Layer::defaults(), Keymap::defaults(), None, Vec::new())
    }
}

impl Config {
    /// Resolve the configuration.
    ///
    /// `flags` is the command line, `get` reads the environment, and the file
    /// is found from `explicit`, then the environment, then the usual place.
    ///
    /// # Errors
    /// Returns an error when a file the reader named cannot be read or is not
    /// valid TOML. A file that is merely from a different version is not an
    /// error.
    pub fn load(
        flags: Layer,
        explicit: Option<&Path>,
        get: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self> {
        Self::load_from(flags, explicit, get, default_path().as_deref())
    }

    /// [`load`](Self::load), with the usual place handed in rather than found.
    ///
    /// The one impure step in this module was `locate` reaching for
    /// `default_path()` on its own, which made "nothing configured" mean
    /// "nothing configured, unless whoever is running this has a file" — so
    /// the tests for the defaults asserted on the developer's own settings and
    /// passed only because most machines have none. Handing the location in
    /// keeps the promise the module header makes.
    fn load_from(
        flags: Layer,
        explicit: Option<&Path>,
        get: &dyn Fn(&str) -> Option<String>,
        default: Option<&Path>,
    ) -> Result<Self> {
        let (environment, mut warnings) = Layer::from_env(get);
        let (file, path) = match locate(explicit, get, default) {
            Some(found) => {
                let (file, unknown) = read(&found)?;
                warnings.extend(unknown.into_iter().map(|key| {
                    format!(
                        "{}: `{key}` is not a setting this version has",
                        found.display()
                    )
                }));
                (file, Some(found))
            }
            None => (File::default(), None),
        };

        let mut keymap = Keymap::defaults();
        warnings.extend(keys::merge(&mut keymap, &file.keys));

        let settings = flags
            .over(environment)
            .over(Layer::from_file(&file))
            .over(Layer::defaults());
        Ok(Self::from_parts(settings, keymap, path, warnings))
    }

    /// Turn a fully resolved layer into settings.
    fn from_parts(
        layer: Layer,
        keymap: Keymap,
        path: Option<PathBuf>,
        warnings: Vec<String>,
    ) -> Self {
        // The defaults layer answers everything except width, so these
        // fallbacks are belt and braces rather than policy.
        Self {
            style: layer
                .style
                .unwrap_or_else(|| ThemeVariant::Slate.name().to_owned()),
            width: layer.width,
            line_numbers: layer.line_numbers.unwrap_or(false),
            mouse: layer.mouse.unwrap_or(true),
            all: layer.all.unwrap_or(false),
            preserve_new_lines: layer.preserve_new_lines.unwrap_or(false),
            update_check: layer.update_check.unwrap_or(true),
            terminal_query: layer.terminal_query.unwrap_or(true),
            theme_watch: layer.theme_watch.unwrap_or_default(),
            contents: layer.contents.unwrap_or(true),
            hints: layer.hints.unwrap_or(true),
            html: layer.html.unwrap_or_default(),
            keymap,
            path,
            warnings,
        }
    }

    /// The settings in force, as a file that would produce them.
    ///
    /// The only practical way to answer "why is this setting what it is?", and
    /// it round-trips: what this prints can be saved as a configuration file.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        match &self.path {
            Some(path) => {
                let _ = writeln!(out, "# Effective configuration, from {}.", path.display());
            }
            None => out.push_str("# Effective configuration. No file was found.\n"),
        }
        out.push_str("\n[general]\n");
        let _ = writeln!(out, "style = {:?}", self.style);
        match self.width {
            Some(width) => {
                let _ = writeln!(out, "width = {width}");
            }
            None => out.push_str("# width = 80    # unset: taken from the terminal\n"),
        }
        let _ = writeln!(out, "line-numbers = {}", self.line_numbers);
        let _ = writeln!(out, "mouse = {}", self.mouse);
        let _ = writeln!(out, "all = {}", self.all);
        let _ = writeln!(out, "preserve-new-lines = {}", self.preserve_new_lines);
        let _ = writeln!(out, "update-check = {}", self.update_check);
        let _ = writeln!(out, "terminal-query = {}", self.terminal_query);
        // Printed even when empty, and as a comment then: `mmd config` is how
        // a reader makes their first file, and a setting that only appears
        // once it is already set is one nobody discovers. Paths come back out
        // absolute because loading expanded them, which is what makes this
        // round-trip — a `~` written here would be expanded a second time to
        // the same place.
        if self.theme_watch.is_empty() {
            out.push_str(
                "\n[theme]\n\
                 # watch = [\"~/.local/state/omarchy/current/theme\"]\n\
                 # Paths whose change means the terminal may have been retinted,\n\
                 # for `--style system` on a desktop that swaps a theme underneath\n\
                 # a window that never loses focus. Regaining focus is already a\n\
                 # trigger, so this is only needed when focus never moves.\n",
            );
        } else {
            let paths: Vec<String> = self
                .theme_watch
                .iter()
                .map(|path| format!("{:?}", path.display().to_string()))
                .collect();
            let _ = writeln!(out, "\n[theme]\nwatch = [{}]", paths.join(", "));
        }
        let _ = writeln!(out, "\n[render]\nhtml = {:?}", self.html.name());
        let _ = writeln!(out, "\n[ui]\ncontents = {}", self.contents);
        let _ = writeln!(out, "hints = {}", self.hints);

        for mode in Mode::ALL {
            let bindings: Vec<_> = self.keymap.bindings(*mode).collect();
            if bindings.is_empty() {
                continue;
            }
            let _ = writeln!(out, "\n[keys.{}]", mode.name());
            for (chord, action) in bindings {
                let _ = writeln!(out, "{:?} = {:?}", chord.to_string(), action.name());
            }
        }
        out
    }
}

/// Find the configuration file: what was asked for on the command line, then
/// what the environment names, then the usual place.
///
/// A file the reader named is returned whether or not it exists, so reading it
/// fails loudly — silently ignoring a `--config` would leave them wondering
/// why nothing they wrote took effect. The default location is returned only
/// if something is there, because most people have no configuration file and
/// that is not a problem.
fn locate(
    explicit: Option<&Path>,
    get: &dyn Fn(&str) -> Option<String>,
    default: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }
    if let Some(path) = get(CONFIG_ENV).filter(|value| !value.trim().is_empty()) {
        return Some(PathBuf::from(path));
    }
    default.filter(|path| path.is_file()).map(Path::to_path_buf)
}

/// Where the configuration file lives when nobody says otherwise.
#[must_use]
pub fn default_path() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("marquee-markdown")
            .join("config.toml"),
    )
}

/// Read and parse a configuration file.
fn read(path: &Path) -> Result<(File, Vec<String>)> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    schema::parse(&text).with_context(|| format!("cannot parse {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        move |name: &str| {
            owned
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        }
    }

    /// The configuration of a machine with no file anywhere.
    ///
    /// Not `Config::load(.., None, &no_env)`: that falls back to the real
    /// `default_path()`, so on a machine that *has* a configuration file these
    /// tests assert on its contents rather than on the defaults. They passed
    /// for as long as they did only because hardly anyone had one — and then
    /// the theme picker started writing it.
    fn nothing_configured() -> Config {
        Config::load_from(Layer::default(), None, &no_env, None).expect("load")
    }

    fn write(text: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, text).expect("write");
        (dir, path)
    }

    #[test]
    fn with_nothing_configured_the_defaults_apply() {
        let config = nothing_configured();
        assert_eq!(config.style, "slate");
        assert!(!config.line_numbers);
        assert!(config.contents);
        assert!(config.warnings.is_empty());
    }

    /// The other half of handing the location in: it must still be read.
    ///
    /// This covers `locate`'s treatment of a default, not `load` passing one —
    /// that single line reaches `dirs`, so nothing here can see it, and a
    /// version of it that passed `None` would leave every reader's
    /// configuration silently unread with the whole suite green. The smoke job
    /// is what stands behind that line: it runs the binary against a
    /// configuration file in the usual place.
    #[test]
    fn the_usual_place_is_read_when_nothing_names_a_file() {
        let (_dir, path) = write("[general]\nstyle = \"paper\"\n");
        let config = Config::load_from(Layer::default(), None, &no_env, Some(&path)).expect("load");
        assert_eq!(config.style, "paper");
        assert_eq!(config.path.as_deref(), Some(path.as_path()));
    }

    /// Unlike a file named with `--config`, one that simply is not there is
    /// the ordinary case rather than an error.
    #[test]
    fn the_usual_place_not_existing_is_not_an_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("config.toml");
        let config =
            Config::load_from(Layer::default(), None, &no_env, Some(&missing)).expect("load");
        assert_eq!(config.style, "slate");
        assert_eq!(config.path, None);
    }

    #[test]
    fn a_named_file_is_read() {
        let (_dir, path) = write("[general]\nstyle = \"paper\"\nwidth = 72\n");
        let config = Config::load(Layer::default(), Some(&path), &no_env).expect("load");
        assert_eq!(config.style, "paper");
        assert_eq!(config.width, Some(72));
        assert_eq!(config.path.as_deref(), Some(path.as_path()));
    }

    #[test]
    fn the_environment_can_name_the_file() {
        let (_dir, path) = write("[general]\nstyle = \"paper\"\n");
        let get = env(&[(CONFIG_ENV, path.to_str().expect("utf8"))]);
        let config = Config::load(Layer::default(), None, &get).expect("load");
        assert_eq!(config.style, "paper");
    }

    #[test]
    fn a_named_file_that_is_missing_is_an_error() {
        // The reader asked for it by name; silently ignoring it would leave
        // them wondering why nothing they wrote took effect.
        let error = Config::load(
            Layer::default(),
            Some(Path::new("/no/such/config.toml")),
            &no_env,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("config.toml"), "{error}");
    }

    #[test]
    fn flags_beat_the_environment_which_beats_the_file() {
        let (_dir, path) = write("[general]\nstyle = \"slate\"\nmouse = true\n");
        let get = env(&[
            (CONFIG_ENV, path.to_str().expect("utf8")),
            ("MARQUEE_STYLE", "paper"),
        ]);
        let flags = Layer {
            width: Some(60),
            ..Layer::default()
        };
        let config = Config::load(flags, None, &get).expect("load");
        assert_eq!(config.width, Some(60), "flags lost");
        assert_eq!(config.style, "paper", "environment lost");
        assert!(config.mouse, "file lost");
    }

    #[test]
    fn a_setting_from_a_newer_version_warns_rather_than_failing() {
        let (_dir, path) = write("[general]\nstyle = \"paper\"\nteleport = true\n");
        let config = Config::load(Layer::default(), Some(&path), &no_env).expect("load");
        assert_eq!(config.style, "paper", "the rest of the file was lost");
        assert_eq!(config.warnings.len(), 1);
        assert!(
            config.warnings[0].contains("teleport"),
            "{:?}",
            config.warnings
        );
    }

    #[test]
    fn a_file_that_is_not_toml_is_an_error() {
        let (_dir, path) = write("this is not [ toml");
        assert!(Config::load(Layer::default(), Some(&path), &no_env).is_err());
    }

    #[test]
    fn key_bindings_from_a_file_take_effect() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let (_dir, path) = write("[keys.document]\nx = \"quit\"\nq = \"none\"\n");
        let config = Config::load(Layer::default(), Some(&path), &no_env).expect("load");
        let press = |code| KeyEvent::new(code, KeyModifiers::NONE);
        assert_eq!(
            config
                .keymap
                .action(Mode::Document, press(KeyCode::Char('x'))),
            Some(crate::app::action::Action::Quit)
        );
        assert_eq!(
            config
                .keymap
                .action(Mode::Document, press(KeyCode::Char('q'))),
            None
        );
    }

    #[test]
    fn a_watched_theme_path_survives_being_printed_and_read_back() {
        // The round-trip above starts from a file with no `[theme]` in it, so
        // it would pass just as well if this setting were never printed —
        // which is exactly how `mmd config > config.toml` comes to lose it.
        let (_dir, path) = write("[theme]\nwatch = [\"/etc/theme-state\"]\n");
        let config = Config::load(Layer::default(), Some(&path), &no_env).expect("load");
        assert_eq!(config.theme_watch, vec![PathBuf::from("/etc/theme-state")]);

        let (_dir2, again) = write(&config.to_toml());
        let reloaded = Config::load(Layer::default(), Some(&again), &no_env).expect("reload");
        assert!(reloaded.warnings.is_empty(), "{:?}", reloaded.warnings);
        assert_eq!(reloaded.theme_watch, config.theme_watch);
    }

    #[test]
    fn the_watch_setting_is_advertised_even_when_it_is_unset() {
        // `mmd config` is how a reader makes their first file. A setting that
        // only appears once it is already set is one nobody discovers.
        let config = Config::default();
        assert!(config.theme_watch.is_empty());
        let printed = config.to_toml();
        assert!(printed.contains("[theme]"), "{printed}");
        assert!(printed.contains("# watch = ["), "{printed}");
    }

    #[test]
    fn the_effective_configuration_round_trips_through_a_file() {
        let (_dir, path) =
            write("[general]\nstyle = \"paper\"\nwidth = 72\n[keys.document]\nx = \"quit\"\n");
        let config = Config::load(Layer::default(), Some(&path), &no_env).expect("load");
        let printed = config.to_toml();

        let (_dir2, again) = write(&printed);
        let reloaded = Config::load(Layer::default(), Some(&again), &no_env).expect("reload");
        assert!(
            reloaded.warnings.is_empty(),
            "what it printed it cannot read back: {:?}",
            reloaded.warnings
        );
        assert_eq!(reloaded.style, config.style);
        assert_eq!(reloaded.width, config.width);
        assert_eq!(reloaded.theme_watch, config.theme_watch);
        // Every chord resolves the same. The order the bindings are stored in
        // differs — a TOML table is sorted — and that only affects how the key
        // reference groups them.
        for mode in Mode::ALL {
            let mut before: Vec<_> = config.keymap.bindings(*mode).collect();
            let mut after: Vec<_> = reloaded.keymap.bindings(*mode).collect();
            before.sort_by_key(|(chord, _)| chord.to_string());
            after.sort_by_key(|(chord, _)| chord.to_string());
            assert_eq!(before, after, "{mode} bindings changed");
        }
    }

    #[test]
    fn the_effective_configuration_says_where_it_came_from() {
        let config = nothing_configured();
        assert!(config.to_toml().contains("No file was found"));

        let (_dir, path) = write("[general]\nstyle = \"paper\"\n");
        let config = Config::load(Layer::default(), Some(&path), &no_env).expect("load");
        assert!(config.to_toml().contains("config.toml"));
    }

    #[test]
    fn an_unset_width_is_shown_as_a_comment_rather_than_a_number() {
        let config = nothing_configured();
        let printed = config.to_toml();
        assert!(printed.contains("# width"), "{printed}");
    }
}
