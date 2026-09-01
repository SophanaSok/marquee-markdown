# Nix

`default.nix` is a nixpkgs-style derivation, written in the shape
`pkgs/by-name/ma/marquee-markdown/package.nix` wants so that submitting it
upstream is a move rather than a rewrite.

## Trying it from a checkout

```sh
nix-build packaging/nix -A marquee-markdown
./result/bin/mmd README.md
```

## The two hashes

`src.hash` and `cargoHash` both start as a row of `A`s, which is the
conventional placeholder: nix will fail the build and print the hash it
actually computed, and that is the value to paste in. They cannot be filled
before the tag exists, for the same reason the Homebrew formula and the AUR
`sha256sums` cannot — which is why all three are bumped after pushing a tag
rather than in the release commit.

`cargoHash` changes whenever `Cargo.lock` does, not only when the version
does.

## Submitting to nixpkgs

The derivation is deliberately free of anything checkout-specific: it fetches
from GitHub by tag. To submit, copy it to
`pkgs/by-name/ma/marquee-markdown/package.nix` in a nixpkgs checkout, fill in
both hashes, and run `nix-build -A marquee-markdown`.
