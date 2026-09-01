# Nix

`default.nix` is a nixpkgs-style derivation, written in the shape
`pkgs/by-name/ma/marquee-markdown/package.nix` wants so that submitting it
upstream is a move rather than a rewrite.

## Trying it from a checkout

`default.nix` is a package *function* — it takes `lib`, `rustPlatform` and the
rest as arguments, because that is the shape nixpkgs calls. So it cannot be
built directly: `nix-build packaging/nix -A marquee-markdown` fails with
`cannot evaluate a function that has an argument without a value ('lib')`.
Something has to supply the arguments, which is `callPackage`:

```sh
nix-build -E 'with import (fetchTarball
  "https://github.com/NixOS/nixpkgs/archive/nixos-unstable.tar.gz") {};
  callPackage ./packaging/nix/default.nix {}'
./result/bin/mmd README.md
```

The pinned tarball rather than `<nixpkgs>` because a fresh install subscribes
to no channels, and `<nixpkgs>` is then a search-path error rather than a
nixpkgs. With a channel configured, `with import <nixpkgs> {};` is the shorter
form of the same thing.

## The two hashes

Both are real as of 0.7.0. They go stale on the next release, and the way to
refresh one is to put the conventional row of `A`s back: nix fails the build
and prints the hash it actually computed, and that is the value to paste in.
Two builds, because the second hash is only reached once the first is right.

They cannot be filled before the tag exists, for the same reason the Homebrew
formula and the AUR `sha256sums` cannot — which is why all three are bumped
after pushing a tag rather than in the release commit.

`cargoHash` changes whenever `Cargo.lock` does — which is every release, since
the version bump edits the lockfile's own entry for this crate, and also any
time a dependency moves. Expect to refresh both hashes each time, not just the
source one.

`src.hash` is a hash of the unpacked source tree, not of the tarball, so it is
not the `sha256` the Homebrew formula and the AUR PKGBUILD carry for the same
tag — those three values are all different and none can be copied from
another.

## Submitting to nixpkgs

The derivation is deliberately free of anything checkout-specific: it fetches
from GitHub by tag. To submit, copy it to
`pkgs/by-name/ma/marquee-markdown/package.nix` in a nixpkgs checkout and run
`nix-build -A marquee-markdown` — which does work with a bare `-A`, because
there the enclosing `default.nix` is the attribute set that calls this file.

Check the hashes against the release being submitted rather than trusting the
ones here, and read the `checkFlags` comment before dropping the skip it
carries: one watch test sees an event inside the nix sandbox that it sees
nowhere else, and that is suppressed rather than understood.
