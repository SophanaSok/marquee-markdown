//! The only place application state is mutated.
//!
//! Keys reach this module already resolved to an [`Action`] by the keymap, so
//! nothing here matches on a key code and rebinding a key needs no change to
//! any of it.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use super::action::Action;
use super::event::Event;
use super::keymap::Mode;
use super::state::{App, Focus, Overlay, Prompt, PromptKind, Screen, ThemePicker};
use crate::theme::{Appearance, Theme, ThemeVariant};

/// Apply one event.
pub fn handle(app: &mut App, event: Event) {
    match event {
        Event::Key(key) => {
            // A message answers the last key; the next key replaces it.
            app.message = None;
            match app.keymap.action(app.mode(), key) {
                Some(action) => apply(app, action),
                // Anything a prompt has not bound is text being typed into it.
                // This is what keeps `q` in a search box from quitting.
                None if app.prompt.is_some() => type_into_prompt(app, key),
                None => {}
            }
        }
        Event::Paste(text) => paste(app, text),
        Event::Mouse(mouse) => mouse_event(app, mouse),
        Event::Scan { generation, scan } => scan_reported(app, generation, scan),
        // The document changed on disk. Silent when it worked: a reader
        // editing in another window does not need to be told each time they
        // save, only when it failed.
        Event::Reload => reload(app, false),
        // Resizes are handled by recomputing pane geometry before the next
        // draw, which happens for every iteration anyway.
        Event::Resize(_, _) => {}
    }
}

/// Apply one action.
pub fn apply(app: &mut App, action: Action) {
    // While the key reference is open, the movement keys move *it* — a page
    // of bindings that silently scrolled the document underneath would be
    // worse than one that ignored the keys.
    if app.overlay == Some(Overlay::Help) && scroll_help(app, action) {
        return;
    }
    let extent = app.extent();
    match action {
        Action::Quit => app.should_quit = true,
        Action::Escape => escape(app),
        Action::ToggleHelp => {
            // Not bound in the picker by default, but it is rebindable, and
            // burying the picker under the key reference would leave a preview
            // applied that nobody accepted.
            if app.overlay == Some(Overlay::Themes) {
                cancel_picker(app);
            }
            app.overlay = match app.overlay {
                Some(Overlay::Help) => None,
                _ => {
                    // A fresh open starts at the top; a reader who scrolled
                    // last time was looking for something else then.
                    app.help_scroll = 0;
                    Some(Overlay::Help)
                }
            }
        }
        // Costs a row of document, so it is the reader's to spend: the key is
        // advertised on the line itself, and `[ui] hints = false` makes the
        // choice stick without this having to write to a file.
        Action::ToggleHints => app.hints = !app.hints,
        Action::ToggleTheme => {
            std::mem::swap(&mut app.theme, &mut app.alternate);
            // The layout cache notices the change on the next reconcile and
            // re-lays out the document, keeping the reading position.
        }
        Action::ThemePicker => toggle_picker(app),
        Action::ThemeDown => move_picker(app, 1),
        Action::ThemeUp => move_picker(app, -1),
        Action::ThemeTop => jump_picker(app, 0),
        Action::ThemeBottom => jump_picker(app, usize::MAX),
        Action::ThemeAccept => accept_theme(app),
        Action::LineDown => app.view.scroll(1, extent),
        Action::LineUp => app.view.scroll(-1, extent),
        Action::HalfPageDown => app.view.half_page(1, extent),
        Action::HalfPageUp => app.view.half_page(-1, extent),
        Action::PageDown => app.view.page(1, extent),
        Action::PageUp => app.view.page(-1, extent),
        Action::Top => app.view.to_top(),
        Action::Bottom => app.view.to_bottom(extent),
        Action::ScrollLeft => app.view.pan(-1, extent),
        Action::ScrollRight => app.view.pan(1, extent),

        Action::ToggleToc => app.toc_visible = !app.toc_visible,
        Action::FocusNext => focus_next(app),
        Action::TocDown => move_cursor(app, 1),
        Action::TocUp => move_cursor(app, -1),
        Action::TocTop => set_cursor(app, app.toc.visible.first().copied()),
        Action::TocBottom => set_cursor(app, app.toc.visible.last().copied()),
        Action::TocCollapse => collapse(app),
        Action::TocExpand => expand(app),
        Action::TocOpen => open_selected(app),

        Action::SearchStart => {
            app.prompt = Some(Prompt {
                kind: PromptKind::Search,
                input: String::new(),
            });
        }
        Action::SearchNext => step_search(app, 1),
        Action::SearchPrevious => step_search(app, -1),
        Action::PromptAccept => accept_prompt(app),
        Action::PromptBackspace => backspace(app),
        Action::PromptClear => {
            if let Some(prompt) = app.prompt.as_mut() {
                prompt.input.clear();
            }
        }

        Action::BrowserDown => with_browser(app, |browser| browser.move_cursor(1)),
        Action::BrowserUp => with_browser(app, |browser| browser.move_cursor(-1)),
        Action::BrowserPageDown => browser_page(app, 1),
        Action::BrowserPageUp => browser_page(app, -1),
        Action::BrowserTop => with_browser(app, crate::browser::Browser::to_first),
        Action::BrowserBottom => with_browser(app, crate::browser::Browser::to_last),
        Action::BrowserOpen => open_selected_file(app),
        Action::Reload => reload(app, true),
        Action::BrowserRescan => rescan(app),
        Action::BrowserToggleHidden => {
            app.options.all = !app.options.all;
            app.message = Some(
                if app.options.all {
                    "also showing hidden and ignored files"
                } else {
                    "hiding hidden and ignored files"
                }
                .to_owned(),
            );
            rescan(app);
        }
        Action::LinkNext => step_link(app, 1),
        Action::LinkPrevious => step_link(app, -1),
        Action::LinkOpen => open_link(app),
        Action::LinkCopy => copy_link(app),
        Action::CopyDocument => copy_document(app),
        Action::Edit => request_edit(app),
        #[cfg(unix)]
        Action::Suspend => app.pending = Some(crate::app::external::Request::Suspend),
        Action::FilterStart => {
            app.prompt = Some(Prompt {
                kind: PromptKind::Filter,
                // Filtering is incremental, so the prompt starts from what is
                // already in force rather than throwing it away.
                input: app
                    .browser
                    .as_ref()
                    .map(|browser| browser.filter.clone())
                    .unwrap_or_default(),
            });
        }
    }
}

/// Do something to the browser, if there is one.
fn with_browser(app: &mut App, change: impl FnOnce(&mut crate::browser::Browser)) {
    if let Some(browser) = app.browser.as_mut() {
        change(browser);
    }
}

/// Move a whole screen through the file list, which is what these keys do in
/// glow's browser even though the same keys move half a screen in its pager.
fn browser_page(app: &mut App, direction: isize) {
    let step = isize::try_from(app.panes.body.height.max(1)).unwrap_or(1);
    with_browser(app, |browser| browser.move_cursor(direction * step));
}

/// Read the selected file.
fn open_selected_file(app: &mut App) {
    let Some(path) = app
        .browser
        .as_ref()
        .and_then(|browser| browser.selected())
        .map(|entry| entry.path.clone())
    else {
        app.message = Some("nothing to open".to_owned());
        return;
    };
    // A browser only ever offers local files, so nothing here reaches the
    // network; the fetcher is inert until something asks it for a URL.
    let fetcher = crate::source::HttpFetcher::new();
    match crate::source::resolve(&crate::source::SourceSpec::File(path.clone()), &fetcher) {
        Ok(source) => app.read(source),
        // A file that vanished mid-scan, or one that is not readable, is not
        // worth ending the session over.
        Err(error) => app.message = Some(format!("cannot open {}: {error}", path.display())),
    }
}

/// Step to the next or previous link, bringing it into view.
fn step_link(app: &mut App, direction: isize) {
    match app.links.step(direction, app.view.top) {
        Some(line) => {
            let extent = app.extent();
            app.view.reveal(line, extent);
        }
        None => app.message = Some("no links in this document".to_owned()),
    }
}

/// Follow the selected link.
///
/// A link to a heading in this document is navigation, not a handoff: the
/// outline already knows where every slug is, and sending `#section` to the
/// system opener does nothing a reader would recognise as following it.
fn open_link(app: &mut App) {
    let Some(target) = selected_target(app) else {
        app.message = Some("press ] to pick a link".to_owned());
        return;
    };
    match target {
        LinkTarget::Anchor(slug) => jump_to_anchor(app, &slug),
        LinkTarget::External(url) => {
            // Opening hands off to another program entirely; that it failed
            // is worth saying, but never worth ending the session over.
            match open::that_detached(&url) {
                Ok(()) => app.message = Some(format!("opening {url}")),
                Err(error) => app.message = Some(format!("cannot open {url}: {error}")),
            }
        }
    }
}

/// Scroll to the heading a `#slug` link names.
fn jump_to_anchor(app: &mut App, slug: &str) {
    let found = app
        .doc
        .doc()
        .outline
        .iter()
        .find(|anchor| anchor.id == slug)
        .map(|anchor| (anchor.line, anchor.text.clone()));
    match found {
        Some((line, text)) => {
            let extent = app.extent();
            app.view.go_to(line, extent);
            app.message = Some(format!("\u{2192} {text}"));
        }
        None => app.message = Some(format!("no heading `#{slug}` in this document")),
    }
}

/// Copy the selected link's address.
fn copy_link(app: &mut App) {
    let Some(target) = selected_target(app) else {
        app.message = Some("press ] to pick a link".to_owned());
        return;
    };
    // An in-document link is copied as the document spells it: `#section` is
    // what belongs in the markdown it came from, and there is no address to
    // give instead.
    let text = match target {
        LinkTarget::Anchor(slug) => format!("#{slug}"),
        LinkTarget::External(url) => url,
    };
    copy(app, &text);
}

/// Where a link goes, once resolved against the document it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LinkTarget {
    /// A heading in this document, by slug.
    Anchor(String),
    /// An address for the system to open.
    External(String),
}

/// The selected link, resolved. `None` when nothing is selected.
fn selected_target(app: &App) -> Option<LinkTarget> {
    let link = app.links.selected_url(app.doc.doc())?;
    Some(resolve_link(app, link))
}

/// Copy the document as it was written, not as it was rendered: what a reader
/// wants to paste elsewhere is the markdown.
fn copy_document(app: &mut App) {
    let text = app.doc.source.text.clone();
    copy(app, &text);
}

/// Put text on the clipboard, reporting either way.
fn copy(app: &mut App, text: &str) {
    match crate::util::clipboard::copy(text) {
        Ok(method) => app.message = Some(method.describe().to_owned()),
        // A clipboard failure must never escape the loop: it can fail for
        // reasons — a headless session, no compositor — that have nothing to
        // do with the document being read.
        Err(error) => app.message = Some(format!("cannot copy: {error}")),
    }
}

/// Turn a link into something openable, resolving a relative one against
/// wherever the document came from.
fn resolve_link(app: &App, link: &str) -> LinkTarget {
    use crate::source::Base;

    if let Some(slug) = link.strip_prefix('#') {
        return LinkTarget::Anchor(slug.to_owned());
    }
    LinkTarget::External(match &app.doc.source.base {
        // Joining is not enough: a root-relative link resolves against the
        // host, and `..` has to be folded away.
        Base::Url(base) => crate::source::remote::join_url(base, link),
        Base::Dir(dir) => dir.join(link).display().to_string(),
        Base::Cwd => link.to_owned(),
    })
}

/// Ask the loop to open the document in an editor, at the line on screen.
fn request_edit(app: &mut App) {
    let Some(path) = app.doc.source.path.clone() else {
        app.message = Some("this document is not a file on this machine".to_owned());
        return;
    };
    let line = app.doc.source_line_of(app.view.top);
    app.pending = Some(crate::app::external::Request::Edit { path, line });
}

/// Re-read the document from disk.
fn reload(app: &mut App, announce: bool) {
    match app.reload_from_disk() {
        Ok(()) if announce => app.message = Some("reloaded".to_owned()),
        Ok(()) => {}
        Err(error) => app.message = Some(format!("cannot reload: {error}")),
    }
}

/// Take a batch of results from the directory walk.
///
/// Reports from a superseded walk are dropped: a rescan clears the list, and
/// a straggling batch from the old walk repopulating it would silently mix
/// two scans — including files the new flags say to hide.
fn scan_reported(app: &mut App, generation: u64, scan: crate::browser::Scan) {
    let Some(browser) = app.browser.as_mut() else {
        return;
    };
    if generation != browser.generation() {
        return;
    }
    match scan {
        crate::browser::Scan::Found(entries) => browser.extend(entries),
        crate::browser::Scan::Done => browser.finish_scan(),
    }
}

/// Start a walk of the browsed directory, reporting into the event queue.
///
/// The one spawn path for the initial scan and every rescan. Headless runs
/// have no queue (`app.events` is `None`), so tests exercise the state
/// changes and feed `Event::Scan` themselves, thread-free.
pub fn respawn_walk(app: &App) {
    let (Some(browser), Some(sender)) = (app.browser.as_ref(), app.events.clone()) else {
        return;
    };
    let generation = browser.generation();
    crate::browser::walk::spawn(browser.root.clone(), app.options.all, move |scan| {
        sender.send(Event::Scan { generation, scan }).is_ok()
    });
}

/// Throw the list away and walk again.
fn rescan(app: &mut App) {
    if let Some(browser) = app.browser.as_mut() {
        browser.begin_rescan();
    }
    respawn_walk(app);
}

/// Move the key reference, if `action` is a movement. The offset is clamped
/// in `derived::sync`, which knows the terminal height; here it only has to
/// move, and saturate at the top.
fn scroll_help(app: &mut App, action: Action) -> bool {
    // A page of the overlay, without knowing its exact height: the clamp in
    // sync trims any overshoot the same frame.
    let page = app.panes.body.height.max(2) / 2;
    let scroll = &mut app.help_scroll;
    match action {
        Action::LineDown => *scroll = scroll.saturating_add(1),
        Action::LineUp => *scroll = scroll.saturating_sub(1),
        Action::HalfPageDown => *scroll = scroll.saturating_add(page / 2),
        Action::HalfPageUp => *scroll = scroll.saturating_sub(page / 2),
        Action::PageDown => *scroll = scroll.saturating_add(page),
        Action::PageUp => *scroll = scroll.saturating_sub(page),
        Action::Top => *scroll = 0,
        Action::Bottom => *scroll = u16::MAX, // clamped to the last row by sync
        _ => return false,
    }
    true
}

/// Open the theme picker, or close it if it is already open.
///
/// The list is read once, here, rather than every frame: it comes off the
/// filesystem, and the draw path may not touch the filesystem.
fn toggle_picker(app: &mut App) {
    if app.overlay == Some(Overlay::Themes) {
        cancel_picker(app);
        return;
    }
    let entries = crate::theme::registry::list();
    if entries.is_empty() {
        // Not reachable while the built-ins are compiled in, but an empty
        // overlay would be a worse answer than saying so.
        app.message = Some("no themes to choose from".to_owned());
        return;
    }
    // Start on the theme in force, so the picker opens showing where you are
    // rather than at the top of an alphabetical list.
    let cursor = entries
        .iter()
        .position(|entry| entry.name == app.theme.name)
        .unwrap_or(0);
    app.picker = Some(ThemePicker {
        entries,
        cursor,
        restore: app.theme.clone(),
        failed: Vec::new(),
    });
    app.overlay = Some(Overlay::Themes);
}

/// Put back the theme the picker opened with, and close it.
///
/// Every move previews, so leaving without this would silently keep whatever
/// the cursor last happened to touch.
fn cancel_picker(app: &mut App) {
    if let Some(picker) = app.picker.take() {
        app.theme = picker.restore;
    }
    app.overlay = None;
}

/// Move the picker's cursor by `delta`, clamped at both ends, and preview.
fn move_picker(app: &mut App, delta: isize) {
    let Some(picker) = app.picker.as_ref() else {
        return;
    };
    let last = picker.entries.len().saturating_sub(1);
    // Clamped rather than wrapping, like the contents pane: a list you can fall
    // off the end of makes it hard to tell where the end is.
    let next = picker.cursor.saturating_add_signed(delta).min(last);
    jump_picker(app, next);
}

/// Put the picker's cursor on `index`, clamped to the list, and preview.
fn jump_picker(app: &mut App, index: usize) {
    let Some(picker) = app.picker.as_mut() else {
        return;
    };
    let index = index.min(picker.entries.len().saturating_sub(1));
    picker.cursor = index;
    preview_theme(app);
}

/// Show the theme under the cursor.
///
/// Assigning the theme is the whole of it: the layout cache notices on the next
/// reconcile and re-lays out, keeping the reading position, exactly as it does
/// for a resize.
fn preview_theme(app: &mut App) {
    let Some(picker) = app.picker.as_ref() else {
        return;
    };
    let Some(entry) = picker.entries.get(picker.cursor) else {
        return;
    };
    let name = entry.name.clone();
    // The terminal was asked once, before the screen was taken; asking again
    // here would put a question to a stream the event thread is reading.
    match crate::theme::registry::resolve(&name, &app.options.terminal) {
        Ok(theme) => app.theme = theme,
        Err(error) => {
            // A theme file somebody wrote can be malformed. Landing the cursor
            // on it says so and leaves the previous theme up, rather than
            // taking the reader down or leaving them wondering why nothing
            // happened.
            app.message = Some(format!("{name}: {error}"));
            if let Some(picker) = app.picker.as_mut()
                && !picker.failed.contains(&name)
            {
                picker.failed.push(name);
            }
        }
    }
}

/// Keep the previewed theme, and write it to the configuration file.
///
/// The theme is already on screen — previewing did that — so accepting is about
/// making it stick. A failed write is reported and nothing else: the reader
/// asked for this theme, and they should get it for this session even if it
/// cannot be recorded for the next one.
fn accept_theme(app: &mut App) {
    let Some(picker) = app.picker.take() else {
        app.overlay = None;
        return;
    };
    app.overlay = None;

    // `T` swaps with the alternate, which was worked out from the theme the
    // reader started with. Picking that same theme would leave both sides of
    // the swap identical and `T` doing nothing at all.
    if app.alternate.appearance == app.theme.appearance {
        app.alternate = Theme::new(match app.theme.appearance {
            Appearance::Light => ThemeVariant::Slate,
            Appearance::Dark => ThemeVariant::Paper,
        });
    }

    let name = match picker.entries.get(picker.cursor) {
        Some(entry) => entry.name.clone(),
        None => return,
    };
    // Nothing to save if the cursor never left a theme that would not load, or
    // it is already what is in force.
    if picker.failed.contains(&name) {
        return;
    }

    app.message = Some(match save_style(app, &name) {
        // The path is dropped here rather than the warning: a long one fills
        // the status bar on its own, and of the two the reader already knows
        // where their configuration lives.
        Ok(_) if app.options.style_overridden => {
            format!("{name} saved, but -s or MARQUEE_STYLE wins next run")
        }
        Ok(path) => format!("{name} saved to {}", path.display()),
        Err(error) => format!("could not save {name}: {error}"),
    });
}

/// Record `name` as the theme to start with, returning where it was written.
fn save_style(app: &App, name: &str) -> anyhow::Result<std::path::PathBuf> {
    let path = crate::config::write::target(app.options.config_path.as_deref())?;
    crate::config::write::set_style(&path, name)?;
    Ok(path)
}

/// Step back out of whatever is innermost.
///
/// The ladder is explicit so that adding a layer later means adding a rung
/// rather than reordering a condition, and so the last rung stays a hint
/// rather than an exit: quitting on a stray escape loses the reader's place.
fn escape(app: &mut App) {
    // The picker previews as the cursor moves, so closing it has to put the
    // theme back. Checked before the overlay is taken, or the state needed to
    // do that is already gone.
    if app.overlay == Some(Overlay::Themes) {
        cancel_picker(app);
        return;
    }
    if app.overlay.take().is_some() {
        return;
    }
    if app.prompt.take().is_some() {
        return;
    }
    if app.focus != Focus::Document {
        app.focus = Focus::Document;
        return;
    }
    if app.search.is_active() {
        app.search.clear();
        return;
    }
    if let Some(browser) = app.browser.as_mut() {
        if !browser.filter.is_empty() {
            browser.filter.clear();
            return;
        }
        if app.screen == Screen::Document {
            app.screen = Screen::Browser;
            return;
        }
    }
    app.message = Some("press q to quit".to_owned());
}

/// Move focus between the document and the contents pane.
fn focus_next(app: &mut App) {
    if app.panes.sidebar.is_none() {
        app.message = Some("the contents pane is hidden; press t to show it".to_owned());
        return;
    }
    app.focus = match app.focus {
        Focus::Document => Focus::Toc,
        Focus::Toc => Focus::Document,
    };
}

/// Move the contents cursor `delta` entries through the rows on show, so a
/// folded section is stepped over rather than into.
fn move_cursor(app: &mut App, delta: isize) {
    let Some(position) = app
        .toc
        .visible
        .iter()
        .position(|&row| row == app.toc.cursor)
    else {
        set_cursor(app, app.toc.visible.first().copied());
        return;
    };
    let next = position
        .saturating_add_signed(delta)
        .min(app.toc.visible.len().saturating_sub(1));
    set_cursor(app, app.toc.visible.get(next).copied());
}

fn set_cursor(app: &mut App, row: Option<usize>) {
    if let Some(row) = row {
        app.toc.cursor = row;
    }
}

/// Fold the selected section, or step out to its parent when there is nothing
/// to fold — the behavior every tree view has, and the reason `h` means this
/// here and something else in the document.
fn collapse(app: &mut App) {
    let cursor = app.toc.cursor;
    let foldable = app
        .doc
        .outline()
        .rows()
        .get(cursor)
        .is_some_and(|row| row.has_children() && !is_collapsed(app, cursor));
    if foldable {
        set_collapsed(app, cursor, true);
    } else if let Some(parent) = app.doc.outline().parent(cursor) {
        app.toc.cursor = parent;
    }
}

/// Unfold the selected section, or step into it when it is already open.
fn expand(app: &mut App) {
    let cursor = app.toc.cursor;
    if is_collapsed(app, cursor) {
        set_collapsed(app, cursor, false);
    } else if let Some(first) = app
        .doc
        .outline()
        .rows()
        .get(cursor)
        .filter(|row| row.has_children())
        .map(|row| row.subtree.start)
    {
        app.toc.cursor = first;
    }
}

fn is_collapsed(app: &App, row: usize) -> bool {
    app.toc.collapsed.get(row).copied().unwrap_or(false)
}

fn set_collapsed(app: &mut App, row: usize, collapsed: bool) {
    let rows = app.doc.outline().len();
    app.toc.collapsed.resize(rows, false);
    if let Some(flag) = app.toc.collapsed.get_mut(row) {
        *flag = collapsed;
    }
}

/// Jump the document to the selected entry and hand focus back to it, which is
/// what choosing an entry is for.
fn open_selected(app: &mut App) {
    let Some(line) = app.anchor_of(app.toc.cursor).map(|anchor| anchor.line) else {
        return;
    };
    let extent = app.extent();
    app.view.go_to(line, extent);
    app.focus = Focus::Document;
}

/// Go to the next or previous hit, bringing it into view.
fn step_search(app: &mut App, direction: isize) {
    if !app.search.is_active() {
        app.message = Some("press / to search".to_owned());
        return;
    }
    let line = if direction >= 0 {
        app.search.select_next()
    } else {
        app.search.select_previous()
    };
    match line {
        Some(line) => {
            let extent = app.extent();
            app.view.reveal(line, extent);
        }
        None => app.message = Some(format!("no match for `{}`", app.search.query())),
    }
}

/// Run what was typed at the prompt.
fn accept_prompt(app: &mut App) {
    let Some(prompt) = app.prompt.take() else {
        return;
    };
    match prompt.kind {
        PromptKind::Filter => {
            if let Some(browser) = app.browser.as_mut() {
                browser.filter = prompt.input;
            }
        }
        PromptKind::Search => {
            if prompt.input.is_empty() {
                app.search.clear();
                return;
            }
            app.search.search(
                app.doc.doc(),
                app.doc.revision(),
                &prompt.input,
                app.view.top,
            );
            match app
                .search
                .current_match()
                .map(crate::doc::search::Match::first_line)
            {
                Some(line) => {
                    let extent = app.extent();
                    app.view.reveal(line, extent);
                }
                None => app.message = Some(format!("no match for `{}`", prompt.input)),
            }
        }
    }
}

/// Delete the last character, cancelling the prompt when there is nothing left
/// — backspacing out of an empty prompt is how a reader expects to leave it.
fn backspace(app: &mut App) {
    let Some(prompt) = app.prompt.as_mut() else {
        return;
    };
    if prompt.input.pop().is_none() {
        app.prompt = None;
    }
}

/// Add a typed character to the open prompt.
///
/// Only unmodified characters: a chord the prompt has not bound is not text.
fn type_into_prompt(app: &mut App, key: KeyEvent) {
    let printable = key.modifiers - KeyModifiers::SHIFT == KeyModifiers::NONE;
    if let (KeyCode::Char(c), true) = (key.code, printable)
        && let Some(prompt) = app.prompt.as_mut()
    {
        prompt.input.push(c);
    }
}

/// Pasted text goes into an open prompt and is ignored otherwise.
fn paste(app: &mut App, mut text: String) {
    let Some(prompt) = app.prompt.as_mut() else {
        return;
    };
    // A paste carrying newlines would otherwise submit itself line by line.
    text.retain(|c| !c.is_control());
    prompt.input.push_str(&text);
}

/// Mouse wheel scrolling, in whatever the movement keys would move.
///
/// Three steps a tick, and the same three on every terminal — which is the
/// point of asking for the wheel at all, rather than a number the terminal
/// picked and multiplied by its own scroll factor on the way past.
///
/// Resolved to an [`Action`] rather than aimed at the view, so the wheel and
/// the keys cannot disagree about which pane is being read. A reader who has
/// tabbed into the contents pane and scrolls means that pane; one with the key
/// reference open means the reference. Aiming it at the document regardless
/// was invisible while the wheel was something you had to ask for, and is not
/// now that it is on by default.
fn mouse_event(app: &mut App, mouse: MouseEvent) {
    if !app.options.mouse {
        return;
    }
    let (down, up) = match app.mode() {
        // The key reference scrolls itself: `apply` hands these to
        // `scroll_help` before the view ever sees them.
        Mode::Help => (Action::LineDown, Action::LineUp),
        Mode::Themes => (Action::ThemeDown, Action::ThemeUp),
        // Anything else, including a prompt, belongs to the pane underneath —
        // a filter being typed at the browser is still the browser.
        _ => match app.pane_mode() {
            Mode::Browser => (Action::BrowserDown, Action::BrowserUp),
            Mode::Toc => (Action::TocDown, Action::TocUp),
            _ => (Action::LineDown, Action::LineUp),
        },
    };
    let action = match mouse.kind {
        MouseEventKind::ScrollDown => down,
        MouseEventKind::ScrollUp => up,
        // Sideways, only where sideways means anything. Panning moves the
        // document, so a list has nothing to do with it, and under an overlay
        // it would move a document nobody can see. `h` and `l` fold the
        // outline rather than panning it, which is not something to do to
        // somebody by accident.
        MouseEventKind::ScrollLeft if pans(app) => Action::ScrollLeft,
        MouseEventKind::ScrollRight if pans(app) => Action::ScrollRight,
        // `event::translate` drops everything else before it reaches the
        // queue; this arm is what makes the match exhaustive for the events a
        // test can still hand straight to `handle`.
        _ => return,
    };
    for _ in 0..WHEEL_STEP {
        apply(app, action);
    }
}

/// How far one tick of the wheel goes, in whatever the pane counts in.
const WHEEL_STEP: usize = 3;

/// Whether a sideways tick has a document to move, with nothing over it.
fn pans(app: &App) -> bool {
    app.pane_mode() == Mode::Document && matches!(app.mode(), Mode::Document | Mode::Prompt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::Options;
    use crate::source::{Base, Source};
    use crate::theme::{Theme, ThemeVariant};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn app() -> App {
        let text: String = (1..=200).map(|n| format!("line {n}\n\n")).collect();
        let mut app = App::new(
            Source::from_text(&text, None, "t.md".into(), Base::Cwd),
            Theme::new(ThemeVariant::Slate),
            Options::default(),
        );
        crate::app::reconcile(&mut app, ratatui::layout::Rect::new(0, 0, 60, 24));
        app
    }

    /// A document with an outline, so the contents pane exists and can be
    /// focused. The plain `app()` above has no headings and no pane.
    fn outlined() -> App {
        let text: String = (1..=40)
            .map(|n| format!("## Section {n}\n\nline {n}\n\n"))
            .collect();
        let mut app = App::new(
            Source::from_text(&text, None, "t.md".into(), Base::Cwd),
            Theme::new(ThemeVariant::Slate),
            Options {
                mouse: true,
                ..Options::default()
            },
        );
        crate::app::reconcile(&mut app, ratatui::layout::Rect::new(0, 0, 100, 24));
        app
    }

    fn wheel(app: &mut App, kind: MouseEventKind) {
        handle(
            app,
            Event::Mouse(MouseEvent {
                kind,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            }),
        );
    }

    fn press(app: &mut App, code: KeyCode) {
        handle(app, Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    #[test]
    fn quitting_sets_the_flag_rather_than_exiting() {
        let mut app = app();
        press(&mut app, KeyCode::Char('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn help_opens_and_closes_and_changes_the_mode_with_it() {
        let mut app = app();
        press(&mut app, KeyCode::Char('?'));
        assert_eq!(app.overlay, Some(Overlay::Help));
        assert_eq!(app.mode(), crate::app::keymap::Mode::Help);
        press(&mut app, KeyCode::Char('?'));
        assert_eq!(app.overlay, None);
    }

    #[test]
    fn q_closes_the_help_overlay_instead_of_quitting() {
        let mut app = app();
        press(&mut app, KeyCode::Char('?'));
        press(&mut app, KeyCode::Char('q'));
        assert_eq!(app.overlay, None);
        assert!(!app.should_quit, "help swallowed the reader's document");
    }

    #[test]
    fn scrolling_keys_do_nothing_while_help_is_open() {
        let mut app = app();
        press(&mut app, KeyCode::Char('j'));
        let top = app.view.top;
        press(&mut app, KeyCode::Char('?'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('G'));
        assert_eq!(app.view.top, top);
    }

    #[test]
    fn escape_with_nothing_open_hints_rather_than_quitting() {
        let mut app = app();
        press(&mut app, KeyCode::Esc);
        assert!(!app.should_quit);
        assert!(app.message.is_some());
        // The next key clears it again.
        press(&mut app, KeyCode::Char('j'));
        assert!(app.message.is_none());
    }

    #[test]
    fn toggling_the_theme_swaps_both_ways() {
        let mut app = app();
        assert_eq!(app.theme.name, "slate");
        press(&mut app, KeyCode::Char('T'));
        assert_eq!(app.theme.name, "paper");
        press(&mut app, KeyCode::Char('T'));
        assert_eq!(app.theme.name, "slate");
    }

    /// The picker lists whatever the registry finds, which on a developer's
    /// machine includes any theme they have installed. These tests therefore
    /// assert about the built-ins, which are always first and always there.
    #[test]
    fn the_picker_opens_on_the_theme_in_force() {
        let mut app = app();
        press(&mut app, KeyCode::Char('s'));
        assert_eq!(app.mode(), Mode::Themes);
        let picker = app.picker.as_ref().expect("the picker is open");
        assert_eq!(picker.entries[picker.cursor].name, "slate");
        // Still slate: opening the list is not a change.
        assert_eq!(app.theme.name, "slate");
    }

    #[test]
    fn moving_the_picker_previews_the_theme_under_the_cursor() {
        let mut app = app();
        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.theme.name, "paper");
    }

    #[test]
    fn the_cursor_stops_at_the_ends_rather_than_wrapping() {
        let mut app = app();
        press(&mut app, KeyCode::Char('s'));
        for _ in 0..10 {
            press(&mut app, KeyCode::Char('k'));
        }
        assert_eq!(app.theme.name, "paper");
        let picker = app.picker.as_ref().expect("the picker is open");
        assert_eq!(picker.cursor, 0);
    }

    #[test]
    fn leaving_the_picker_puts_the_theme_back() {
        let mut app = app();
        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.theme.name, "paper");
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.theme.name, "slate", "escape did not restore the theme");
        assert!(app.picker.is_none());
        assert_eq!(app.mode(), Mode::Document);
    }

    #[test]
    fn q_leaves_the_picker_rather_than_quitting() {
        let mut app = app();
        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Char('k'));
        press(&mut app, KeyCode::Char('q'));
        assert!(!app.should_quit);
        assert_eq!(app.theme.name, "slate");
    }

    #[test]
    fn pressing_the_picker_key_again_closes_it_without_keeping_the_preview() {
        let mut app = app();
        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Char('k'));
        press(&mut app, KeyCode::Char('s'));
        assert!(app.picker.is_none());
        assert_eq!(app.theme.name, "slate");
    }

    #[test]
    fn accepting_keeps_the_theme_and_records_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# mine\n[general]\nwidth = 72\n").expect("write");

        let mut app = app();
        app.options.config_path = Some(path.clone());
        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Char('k'));
        press(&mut app, KeyCode::Enter);

        assert_eq!(app.theme.name, "paper");
        assert!(app.picker.is_none());
        assert_eq!(app.mode(), Mode::Document);

        let written = std::fs::read_to_string(&path).expect("read back");
        assert!(written.contains("style = \"paper\""), "{written}");
        assert!(written.contains("# mine"), "{written}");
        assert!(written.contains("width = 72"), "{written}");
    }

    #[test]
    fn picking_the_theme_t_would_have_swapped_to_keeps_t_working() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut app = app();
        app.options.config_path = Some(dir.path().join("config.toml"));
        // The reader starts on slate, so `T` would go to paper. Pick paper.
        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Char('k'));
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.theme.name, "paper");

        // Without fixing up the alternate, both sides of the swap are paper
        // and `T` silently does nothing.
        press(&mut app, KeyCode::Char('T'));
        assert_eq!(app.theme.name, "slate");
        press(&mut app, KeyCode::Char('T'));
        assert_eq!(app.theme.name, "paper");
    }

    #[test]
    fn a_save_that_cannot_be_written_says_so_and_keeps_the_theme() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A directory where the file should be: it cannot be replaced.
        let path = dir.path().join("config.toml");
        std::fs::create_dir(&path).expect("mkdir");

        let mut app = app();
        app.options.config_path = Some(path);
        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Char('k'));
        press(&mut app, KeyCode::Enter);

        // The reader asked for this theme; a failure to record it for next
        // time is not a reason to take it away now.
        assert_eq!(app.theme.name, "paper");
        let message = app.message.as_deref().unwrap_or_default();
        assert!(message.contains("could not save"), "{message}");
    }

    #[test]
    fn a_shadowed_save_says_the_flag_will_still_win() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");

        let mut app = app();
        app.options.config_path = Some(path);
        app.options.style_overridden = true;
        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Char('k'));
        press(&mut app, KeyCode::Enter);

        let message = app.message.as_deref().unwrap_or_default();
        assert!(message.contains("MARQUEE_STYLE"), "{message}");
    }

    #[test]
    fn the_wheel_is_ignored_unless_mouse_support_was_asked_for() {
        let mut app = app();
        let wheel = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        handle(&mut app, Event::Mouse(wheel));
        assert_eq!(app.view.top, 0);

        app.options.mouse = true;
        handle(&mut app, Event::Mouse(wheel));
        assert_eq!(app.view.top, 3);
    }

    #[test]
    fn the_wheel_moves_the_pane_that_has_the_keys() {
        let mut app = outlined();
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.mode(), Mode::Toc, "the contents pane never took focus");

        wheel(&mut app, MouseEventKind::ScrollDown);
        assert_eq!(app.toc.cursor, 3, "the wheel did not reach the outline");
        assert_eq!(
            app.view.top, 0,
            "the wheel moved the document out from under a reader who was in \
             the contents pane"
        );

        press(&mut app, KeyCode::Tab);
        wheel(&mut app, MouseEventKind::ScrollDown);
        assert_eq!(app.view.top, 3, "focus came back but the wheel did not");
        assert_eq!(app.toc.cursor, 3, "the outline moved without being asked");
    }

    #[test]
    fn the_wheel_moves_the_file_list_when_that_is_what_is_on_show() {
        use crate::browser::{Entry, Scan};

        let mut app = App::browsing(
            "/nowhere".into(),
            Theme::new(ThemeVariant::Slate),
            Options {
                mouse: true,
                ..Options::default()
            },
        );
        // Reported rather than walked: the walk needs a directory and a
        // thread, and neither is what this is about.
        let entries = (0..10)
            .map(|n| Entry {
                path: format!("/nowhere/{n}.md").into(),
                display: format!("{n}.md"),
                modified: None,
            })
            .collect();
        handle(
            &mut app,
            Event::Scan {
                generation: 0,
                scan: Scan::Found(entries),
            },
        );
        crate::app::reconcile(&mut app, ratatui::layout::Rect::new(0, 0, 80, 24));
        wheel(&mut app, MouseEventKind::ScrollDown);
        let browser = app.browser.as_ref().expect("the browser");
        assert_eq!(browser.cursor(), 3, "the wheel did not reach the file list");
    }

    #[test]
    fn the_wheel_scrolls_the_key_reference_rather_than_the_page_behind_it() {
        let mut app = outlined();
        press(&mut app, KeyCode::Char('?'));
        wheel(&mut app, MouseEventKind::ScrollDown);
        assert_eq!(app.help_scroll, 3);
        assert_eq!(app.view.top, 0, "the document scrolled under the overlay");
    }

    #[test]
    fn the_wheel_moves_the_theme_picker_while_it_is_open() {
        let mut app = outlined();
        press(&mut app, KeyCode::Char('s'));
        wheel(&mut app, MouseEventKind::ScrollDown);
        let picker = app.picker.as_ref().expect("the picker closed");
        // The picker opens on the theme in force, not at the top, so one tick
        // lands `WHEEL_STEP` below *that* — clamped to the last row.
        //
        // Written as `3.min(len - 1)` this passed for the wrong reason while
        // only three themes shipped: the clamp hid the opening offset, and
        // the first bundled palettes turned a green test red.
        // `restore` is the theme the picker opened with, by definition.
        let opened_at = picker
            .entries
            .iter()
            .position(|e| e.name == picker.restore.name)
            .unwrap_or(0);
        assert_eq!(
            picker.cursor,
            (opened_at + WHEEL_STEP).min(picker.entries.len() - 1)
        );
        assert_eq!(app.view.top, 0);
    }

    #[test]
    fn a_sideways_tick_leaves_a_list_alone() {
        // `h` and `l` fold the outline. A wheel tilted by accident must not
        // collapse the section somebody was reading.
        let mut app = outlined();
        press(&mut app, KeyCode::Tab);
        let before = app.toc.visible.len();
        wheel(&mut app, MouseEventKind::ScrollLeft);
        wheel(&mut app, MouseEventKind::ScrollRight);
        assert_eq!(app.toc.visible.len(), before, "the outline folded");
        assert_eq!(app.view.left, 0, "the document panned from another pane");
    }

    #[test]
    fn an_unbound_key_is_simply_ignored() {
        let untouched = app().summary();
        let mut app = app();
        press(&mut app, KeyCode::Char('z'));
        assert_eq!(app.summary(), untouched);
    }
}
