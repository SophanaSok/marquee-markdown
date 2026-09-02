# Packaging

What is here, and what fills in the blanks.

| File | Filled in by |
| --- | --- |
| `homebrew/marquee-markdown.rb` | `brew bump-formula-pr`, from the tag's source tarball |
| `scoop/marquee-markdown.template.json` | the release workflow, from `checksums.txt` |
| `aur/marquee-markdown/PKGBUILD` | by hand after the tag, from the source tarball |
| `aur/marquee-markdown-bin/PKGBUILD` | by hand after the tag, from `checksums.txt` |
| `nix/default.nix` | by hand after the tag; nix prints both hashes |

Two of the four are now published, at 0.9.0:

| Channel | Where |
| --- | --- |
| Homebrew | [SophanaSok/homebrew-marquee](https://github.com/SophanaSok/homebrew-marquee) |
| Scoop | [SophanaSok/scoop-marquee](https://github.com/SophanaSok/scoop-marquee) |

The other two are not published, for different reasons:

- **nixpkgs** is submitted and in review:
  [NixOS/nixpkgs#558998](https://github.com/NixOS/nixpkgs/pull/558998), opened
  as a draft. nixpkgs requires a `meta.maintainers` entry, so the pull request
  carries the package and the `maintainers/maintainer-list.nix` addition
  together. Its [automation/AI policy] requires an `Assisted-by:` trailer on
  any LLM-assisted commit and says a `Co-authored-by:` trailer does not
  satisfy it.

- **The AUR has nowhere to publish to.** It has been closed to new accounts
  since 15 June 2026, after a supply-chain attack that hijacked more than a
  thousand packages to ship credential stealers; package adoption is disabled
  as well, and neither has an announced reopening date. Both PKGBUILDs are
  written, build under `makepkg` and pass `namcap`, and are one `git push`
  each the day it reopens. Do not go looking for a way around this — it is an
  active security control, not a queue.

[automation/AI policy]: https://github.com/NixOS/nixpkgs/blob/master/CONTRIBUTING.md#automationai-policy

What each remaining channel needs, when it can be done:

| Channel | What to do | Then |
| --- | --- | --- |
| AUR | register an SSH key with an AUR account, once registration reopens | `git push` each PKGBUILD to `ssh://aur@aur.archlinux.org/<name>.git`, with a `.SRCINFO` from `makepkg --printsrcinfo` |
| nixpkgs | review the draft and mark it ready | the policy holds the submitter accountable for the contribution and for answering reviewers directly |
| homebrew-core | submit once the project clears its notability bar — for a self-submission that is **90 forks, 90 watchers, or 225 stars, and a repository at least 30 days old** ([Package Acceptance Policy](https://github.com/Homebrew/brew/blob/master/docs/Package-Acceptance-Policy.md#notability); `brew audit --strict --new --online` checks both) | the tap stops being the only route |

The AUR pair is deliberately two packages: `marquee-markdown` builds from
source and runs the test suite, `marquee-markdown-bin` unpacks the release
archive for anyone who does not want a Rust toolchain to read a markdown file.
They conflict, because both install the same two binaries.

`nix/README.md` covers the two placeholder hashes and why they cannot be
filled before the tag.

The Scoop manifest is **not** kept here — only the template is. A manifest
pins a hash, and a hash cannot exist before the archive it describes, so a
checked-in one is stale from the moment a release is tagged until somebody
remembers to move it. It sat at 0.1.0 through two releases that way. The
release workflow now fills the template from the `checksums.txt` it has just
written and attaches `marquee-markdown.json` to the release, so the manifest
for a version is built by the run that builds that version and cannot
describe a different one. To start a bucket, take that asset.

`tests/docs.rs` guards the contract between the two: that the placeholders
the workflow substitutes are there, that filling them yields valid JSON with
the version in its url and `extract_dir`, and that Scoop's own `$version` in
the `autoupdate` block — which keeps a bucket current on its own — is not
one of them.

Debian and RPM metadata live in `Cargo.toml` under `[package.metadata.deb]` and
`[package.metadata.generate-rpm]`; the release workflow builds both with
`cargo deb` and `cargo generate-rpm` and attaches them to the release. Their
asset lists point into `dist/`, which the workflow fills first; to build one
locally, generate those files the same way:

```sh
cargo build --release --locked
mkdir -p dist/man dist/completions
for name in marquee-markdown mmd; do
  target/release/$name man > dist/man/$name.1
  for shell in bash zsh fish; do
    target/release/$name completion $shell > dist/completions/$name.$shell
  done
done
gzip -9n dist/man/*.1
cargo deb --no-build   # or: cargo generate-rpm
```

Both packages are for the release page, not for the distributions' own
archives, and three gaps mark that boundary honestly rather than pretending
otherwise:

- They are built on the Ubuntu runner, so `$auto` resolves the Debian
  dependency against that runner's glibc — the `.deb` installs on releases at
  least as new as it, not on older Debian stable. Building in a Debian stable
  or Fedora container is the fix if either ever needs to reach further back.
- The `.deb` carries no `changelog.Debian.gz` (Policy §12.7): the project
  changelog is Keep-a-Changelog, not Debian format, and maintaining a second
  changelog for one lintian tag is not worth the drift risk.
- `cargo generate-rpm` cannot mark `LICENSE` with `%license` or expand
  `%{?dist}` in `release`, both of which a Fedora package review requires. A
  Fedora submission would need a real spec file, not this pipeline.

The man page and the shell completions are **generated by the binary**
(`marquee-markdown man`, `marquee-markdown completion <shell>`) rather than
checked in, so they cannot drift from the flags the program actually accepts.
The release workflow generates them into every archive and into the Debian
and RPM packages, where they land in the paths the distributions document
(`man1`, `bash-completion/completions`, zsh's `vendor-completions` /
`site-functions`, fish's `vendor_completions.d`).

## The screenshots

Every image in the README is real output, captured by running a release build
on a pseudo-terminal and drawing the resulting cell grid as SVG. Regenerate all
of them at once when the rendering changes visibly:

```sh
cargo build --release
python3 scripts/screenshot.py --all --strict
```

| Image | What it is |
| --- | --- |
| `docs/screenshot.svg` | the reader, `slate`, with the contents pane — under "What it does", since the GIF took the top |
| `docs/screenshot-paper.svg` | the same document in `paper` |
| `docs/screenshot-search.svg` | the same document with `/the` typed |
| `docs/screenshot-browser.svg` | the file browser over the repository root |
| `docs/compare-glow.svg` | `docs/compare.md` through glow and through this |

`--strict` turns two warnings into failures: anything the program wrote to
standard error, and any character that ran past the right edge of the capture.
Both mean the picture is wrong, and both are easy to miss by eye.

Two of these need care:

- **The browser shot is not reproducible.** It lists whatever markdown is in
  the tree, with relative modification times, so it changes whenever the repo
  does. That is fine for a picture and unfit for a test — never gate CI on it.
- **The comparison has to stay fair**, and the script enforces that rather than
  claiming it in a caption: one document, `--config /dev/null` on both sides so
  no local configuration is photographed, an allow-listed environment, an
  80-column terminal for both, no style or width flags, nothing cropped, and
  versions read from the binaries at capture time. If you change any of that,
  change the caption in the image too.

Check `python3 scripts/screenshot.py --self-test` after touching the
escape-sequence parser — it asserts the 256-colour, truecolor, 16-colour and
attribute cases both programs actually emit.

## The demo GIF

`docs/demo.tape` is a [VHS](https://github.com/charmbracelet/vhs) script, and
`docs/demo.gif` is what it produces:

```sh
cargo build --release
vhs docs/demo.tape
```

It records only the moving parts — the contents pane tracking the scroll,
folding, search narrowing as you type, the theme picker previewing against the
document behind it. Everything that photographs fine as a still belongs in a
screenshot instead, where `--strict` can check it.

**This is not gated in CI, deliberately.** A GIF is a lossy re-encode: two runs
of the same tape do not produce identical bytes, so a byte-comparison would be
flaky rather than protective, and a pixel comparison would need a tolerance
nobody can justify. Re-record it by hand when the reader visibly changes, the
same as the screenshots — the difference is that nothing will tell you, so it
is worth checking when a release changes how the reader looks.

The GIF is tracked, and it is the README's hero image. Two
consequences: a stale one is on the front page rather than on somebody's
laptop, and every re-recording is a new megabyte in history rather than a diff.
It is excluded from the published crate in `Cargo.toml` — `docs/` otherwise
ships inside it, and the GIF alone took the packaged crate from 374 KiB to
1.4 MiB compressed for a file no `cargo install` can play.

## Cutting a release

1. Update `CHANGELOG.md`: turn `[Unreleased]` into the version and the date,
   and add the compare link at the bottom. The release workflow takes the
   notes from that section by matching the version, so the heading has to
   contain it — and refuses to release when it finds none.
2. Bump `version` in `Cargo.toml`, and run `cargo check` so `Cargo.lock`
   follows.
3. Commit, tag `vX.Y.Z`, and push the tag.
4. Once the tag is up, bump the four manifests that pin a version and a hash:

   - `homebrew/marquee-markdown.rb`, with `brew bump-formula-pr --version=X.Y.Z`
   - `aur/marquee-markdown/PKGBUILD` — `pkgver`, and `sha256sums` from
     `makepkg -g`
   - `aur/marquee-markdown-bin/PKGBUILD` — `pkgver`, and `sha256sums_x86_64`
     from the release's `checksums.txt`
   - `nix/default.nix` — `version`, then let nix print `src.hash` and
     `cargoHash` and paste them back

   These are last rather than part of the release commit because each pins the
   hash of the tag's source tarball, which does not exist until the tag is
   pushed — a correct hash inside the release commit would be the hash of a
   tree containing itself.

   `tests/docs.rs` allows each to be one release behind the newest dated
   heading in the changelog — that gap is this step — and fails at two, so
   skipping one shows up as a red test on the *next* release rather than as an
   install of the wrong version. `the_homebrew_formula_points_at_a_real_release`
   covers the formula, which also has a hash to check;
   `every_pinned_package_manifest_points_at_a_real_release` covers the other
   three.

Steps 1 to 3 are the whole of the release itself. The workflow does the
rest, in an order that cannot leave the two sides disagreeing: it first
refuses a tag that does not match `Cargo.toml` or a changelog with no notes
for the version, then builds the archives, the Debian and RPM packages, and
the checksums, then builds the Scoop manifest from those checksums, then
publishes the crate to crates.io, and only then publishes the GitHub release —
so a release page never exists for a version crates.io does not have. A failed job is rerun with "re-run failed
jobs"; a publish that already succeeded is not run twice.

Publishing authenticates over GitHub OIDC ([trusted
publishing](https://crates.io/docs/trusted-publishing)) rather than a stored
token. The one-time setup lives on crates.io under the crate's Settings →
Trusted Publishing: add a GitHub publisher with repository owner
`SophanaSok`, repository `marquee-markdown`, workflow file `release.yml`.
Until that is configured, the publish job fails at authentication and can be
rerun once it is.

Both `marquee-markdown` and `mmd` are shipped, as two real binaries rather than
a binary and a symlink. That costs about 12 MB, and buys the same result from
every install method — `cargo install` cannot make a symlink, so anything else
would mean the alias existing in some installs and not others.
