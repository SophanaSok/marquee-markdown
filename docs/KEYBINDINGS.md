# Keybindings

Generated from the default keymap — do not edit by hand. Regenerate with:

```sh
cargo run -- keys > docs/KEYBINDINGS.md
```

Keys are written the way a configuration file spells them, so anything
here can be pasted into `[keys.<mode>]`.

## `document`

| Key | Action | |
| --- | --- | --- |
| `j` `down` | `line-down` | down a line |
| `k` `up` | `line-up` | up a line |
| `d` `ctrl+d` | `half-page-down` | down half a page |
| `u` `ctrl+u` | `half-page-up` | up half a page |
| `f` `space` `pgdn` | `page-down` | down a page |
| `b` `pgup` | `page-up` | up a page |
| `g` `home` | `top` | go to top |
| `G` `end` | `bottom` | go to bottom |
| `h` `left` | `scroll-left` | scroll left |
| `l` `right` | `scroll-right` | scroll right |
| `/` | `search-start` | search |
| `n` | `search-next` | next hit |
| `N` | `search-previous` | previous hit |
| `]` | `link-next` | next link |
| `[` | `link-previous` | previous link |
| `enter` | `link-open` | open the link |
| `y` | `link-copy` | copy the link |
| `c` | `copy-document` | copy the document |
| `e` | `edit` | edit this document |
| `r` | `reload` | reload from disk |
| `t` | `toggle-toc` | show / hide contents |
| `tab` | `focus-next` | focus contents / document |
| `T` | `toggle-theme` | switch light / dark |
| `s` | `theme-picker` | choose a theme |
| `?` | `toggle-help` | toggle this help |
| `H` | `toggle-hints` | show / hide the hint line |
| `esc` | `escape` | close overlay |
| `q` `ctrl+c` | `quit` | quit |
| `ctrl+z` | `suspend` | suspend to the shell |

## `browser`

| Key | Action | |
| --- | --- | --- |
| `j` `down` | `browser-down` | next file |
| `k` `up` | `browser-up` | previous file |
| `l` `right` `pgdn` `f` `d` | `browser-page-down` | next page |
| `h` `left` `pgup` `b` `u` | `browser-page-up` | previous page |
| `g` `home` | `browser-top` | first file |
| `G` `end` | `browser-bottom` | last file |
| `enter` | `browser-open` | read this file |
| `/` | `filter-start` | filter the list |
| `r` | `browser-rescan` | rescan the directory |
| `.` | `browser-toggle-hidden` | show / hide hidden files |
| `T` | `toggle-theme` | switch light / dark |
| `s` | `theme-picker` | choose a theme |
| `?` | `toggle-help` | toggle this help |
| `H` | `toggle-hints` | show / hide the hint line |
| `esc` | `escape` | close overlay |
| `q` `ctrl+c` | `quit` | quit |

## `toc`

| Key | Action | |
| --- | --- | --- |
| `j` `down` | `toc-down` | next entry |
| `k` `up` | `toc-up` | previous entry |
| `g` `home` | `toc-top` | first entry |
| `G` `end` | `toc-bottom` | last entry |
| `h` `left` | `toc-collapse` | fold section |
| `l` `right` | `toc-expand` | unfold section |
| `enter` | `toc-open` | go to entry |
| `tab` | `focus-next` | focus contents / document |
| `t` | `toggle-toc` | show / hide contents |
| `/` | `search-start` | search |
| `T` | `toggle-theme` | switch light / dark |
| `s` | `theme-picker` | choose a theme |
| `?` | `toggle-help` | toggle this help |
| `H` | `toggle-hints` | show / hide the hint line |
| `esc` | `escape` | close overlay |
| `q` `ctrl+c` | `quit` | quit |

## `prompt`

| Key | Action | |
| --- | --- | --- |
| `enter` | `prompt-accept` | accept |
| `backspace` | `prompt-backspace` | delete back |
| `ctrl+u` | `prompt-clear` | clear |
| `esc` | `escape` | close overlay |
| `ctrl+c` | `quit` | quit |

## `help`

| Key | Action | |
| --- | --- | --- |
| `j` `down` | `line-down` | down a line |
| `k` `up` | `line-up` | up a line |
| `d` | `half-page-down` | down half a page |
| `u` | `half-page-up` | up half a page |
| `f` `space` `pgdn` | `page-down` | down a page |
| `b` `pgup` | `page-up` | up a page |
| `g` `home` | `top` | go to top |
| `G` `end` | `bottom` | go to bottom |
| `?` | `toggle-help` | toggle this help |
| `esc` `q` | `escape` | close overlay |
| `ctrl+c` | `quit` | quit |

## `themes`

| Key | Action | |
| --- | --- | --- |
| `j` `down` | `theme-down` | next theme |
| `k` `up` | `theme-up` | previous theme |
| `g` `home` | `theme-top` | first theme |
| `G` `end` | `theme-bottom` | last theme |
| `enter` | `theme-accept` | use this theme |
| `s` | `theme-picker` | choose a theme |
| `esc` `q` | `escape` | close overlay |
| `ctrl+c` | `quit` | quit |
