# Installing ckan with Homebrew

`ckan.rb` is the Homebrew formula for ckan. It lives in this repo under
`HomebrewFormula/`, and it is published to a dedicated tap at
`github.com/JavierAbrego/homebrew-tap`.

## Installation

    brew install JavierAbrego/tap/ckan

The three-component form `user/tap/formula` auto-taps
`JavierAbrego/homebrew-tap`, then installs the formula. It downloads the
prebuilt release tarball for your OS/arch (macOS arm64, macOS Intel or Linux
x86_64) and verifies its sha256.

The explicit two-step form works too:

    brew tap JavierAbrego/tap
    brew install ckan

## Publishing the formula to the tap

Homebrew's short `user/tap/formula` form requires a repo named
`homebrew-<tap>`, so the formula is served from
`github.com/JavierAbrego/homebrew-tap`. To publish it, copy this repo's
`HomebrewFormula/ckan.rb` into that tap as `Formula/ckan.rb`. The `.rb` file
itself does not change.

## Before it works: fill in the sha256

The formula ships with `REPLACE_WITH_SHA256_OF_...` placeholders in its three
`sha256` blocks. After the first release (`git push` of a tag `v0.1.0`), each
release tarball comes with an accompanying `ckan-v0.1.0-<target>.tar.gz.sha256`
file. Copy the hex hash from each into the matching `sha256`:

- aarch64-apple-darwin      -> `on_macos` block / `Hardware::CPU.arm?`
- x86_64-apple-darwin       -> `on_macos` block / else
- x86_64-unknown-linux-gnu  -> `on_linux` block

Until all three are filled in, `brew install` fails the checksum verification.

## Local verification done

- `ruby -c HomebrewFormula/ckan.rb` -> `Syntax OK`.
- The three URLs match EXACTLY the release naming contract.
- `cargo build --release` still builds without warnings; `src/` untouched.

## Deferred (not verifiable locally, needs the published release)

- Real `brew install`: downloads the tarball from GitHub Releases.
- Real sha256 values: depend on the artifacts produced by the release workflow.
- `brew audit` / `brew test`: require Homebrew installed and the release live.
