# hasheus / cli

verify the integrity of a file with the largest decentralized web of trust, from
the command line. the cli version of [hasheus](https://github.com/tinfoilhash/hasheus),
built with [rust](https://www.rust-lang.org/),
[nostr-sdk](https://github.com/rust-nostr/nostr), and
[clap](https://github.com/clap-rs/clap).

## install

download the binary for your platform from the
[latest release](https://github.com/tinfoilhash/hasheus-cli/releases/latest),
make it executable, and put it somewhere on your `PATH` as `hasheus`:

| platform              | binary                  |
| --------------------- | ----------------------- |
| macos (apple silicon) | `hasheus-darwin-arm64`  |
| macos (intel)         | `hasheus-darwin-x86_64` |
| linux (x86-64)        | `hasheus-linux-x86_64`  |
| linux (arm64)         | `hasheus-linux-aarch64` |

each is named after what `uname` prints, so on macos or linux this picks the
right one:

```bash
curl -fL -o hasheus "https://github.com/tinfoilhash/hasheus-cli/releases/latest/download/hasheus-$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)"
chmod +x hasheus
sudo mv hasheus /usr/local/bin/
```

the binaries aren't notarized, so macos won't open one downloaded in a browser,
though `curl` doesn't flag it. for a browser download, remove the quarantine
flag before moving it:

```bash
xattr -d com.apple.quarantine hasheus-darwin-arm64
```

each binary is verified with hasheus, so check yours before running it: drop it
on [hasheus.com](https://hasheus.com), which hashes it in your browser.

### from source

with [rust](https://www.rust-lang.org/tools/install), build it from source,
which is the only way on windows for now:

```bash
cargo install --locked --git https://github.com/tinfoilhash/hasheus-cli
```

## usage

these are the common cases. `hasheus help <command>` covers every option.

### verify

`verify` uses the web of trust of `HASHEUS_PUBLIC_KEY` (your npub). verify a
file, or its sha-256 hash:

```bash
hasheus verify bitcoin.pdf
hasheus verify b1674191a88ec5cdd733e4240a81803105dc412d6c6708d53ab94fc248f4f553
```

pass an npub instead, like your own without setting `HASHEUS_PUBLIC_KEY`, or
someone else's to use their web of trust:

```bash
hasheus verify bitcoin.pdf --user npub1…
```

check its status in a script:

```bash
hasheus verify bitcoin.pdf --json | jq -r .verification.status
```

### publish

`publish` signs with `HASHEUS_SECRET_KEY` (an nsec or ncryptsec), or asks for
it. publish your own verification of a file, with where it can be downloaded:

```bash
hasheus publish bitcoin.pdf --url https://bitcoin.org/bitcoin.pdf
```

publish from a script, without being asked first:

```bash
hasheus publish bitcoin.pdf --url https://bitcoin.org/bitcoin.pdf --yes --json
```

## development

### prerequisites

1. the version of rust pinned in
   [`rust-toolchain.toml`](rust-toolchain.toml). [rustup](https://rustup.rs/)
   installs it, along with clippy, rustfmt, and the llvm tools for coverage:

   ```bash
   rustup toolchain install
   ```

2. a nostr relay for local development. any will work, but a good option is
   built into [nak](https://github.com/fiatjaf/nak), which can also load the
   [sample data](#sample-data):

   ```bash
   nak serve
   ```

### config

configuration is done through environment variables, which default to the
production relays:

- `HASHEUS_FILE_RELAY_URLS`: comma-separated relays to read verifications from.
- `HASHEUS_USER_RELAY_URLS`: comma-separated relays to read profiles and follow
  lists from.

point both at `ws://localhost:10547` to use `nak serve`.

your profile and follow list are cached for an hour, like on the web, in
`hasheus/users` under your cache directory. delete it to fetch them again
sooner.

### sample data

the [web app's repo](https://github.com/tinfoilhash/hasheus) has sample data for
local testing: a relay seed, and a file to verify for each case. with that repo
cloned next to this one, start a relay with it:

```bash
nak serve --events ../hasheus/dev/seed.jsonl
```

then point the cli at that relay as the sample dev user, who follows alice,
bob, and carol, and verify or publish the sample files:

```bash
source ../hasheus/dev/cli.env
cargo run -- verify ../hasheus/dev/files/trusted.txt
```

see the web app's [readme](https://github.com/tinfoilhash/hasheus#sample-data)
for what each file shows, and the keys of every user. pass one to `--user` to
act as them instead.

### tests

the fetcher's tests run against nostr-sdk's in-process relay, so nothing needs
to be running:

```bash
cargo test
```

[cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) measures how much
of the code they cover. install it once, then print a summary per file, or swap
`--summary-only` for `--open` to open an html report:

```bash
cargo install cargo-llvm-cov --locked
cargo llvm-cov --summary-only
```

ci runs the linters, tests, and build on every push to `development` and `main`
and every pull request, and adds the coverage table to each run's summary.

### code style & linting

[rustfmt](https://github.com/rust-lang/rustfmt) enforces a consistent code
style, and [clippy](https://github.com/rust-lang/rust-clippy) enforces a level
of code quality. it's recommended to
[add rust-analyzer to your editor](https://rust-analyzer.github.io/book/other_editors.html)
and format on save.

to run both via the command line:

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings
```

## releasing

releases are built on a mac, for macos and linux, by
[`bin/release`](bin/release). it needs [homebrew](https://brew.sh/)'s llvm to
build for linux, and the [github cli](https://cli.github.com/), logged in, to
create the release:

```bash
brew install llvm gh
gh auth login
```

it also verifies each binary with hasheus itself, so set `HASHEUS_SECRET_KEY` to
the key that signs the verifications (see [publish](#publish)). an ncryptsec
asks for its password for each one, 4 times in all.

when the `development` branch is ready for release, bump the version in
[`Cargo.toml`](Cargo.toml), then build to update `Cargo.lock` to match:

```bash
cargo build
```

commit it as the bare tag, then tag it and push both:

```bash
git commit Cargo.toml Cargo.lock --message v0.2.0
git tag v0.2.0
git push origin development v0.2.0
```

then build every platform, create the github release with them and their
`SHA256SUMS`, and publish a verification of each:

```bash
bin/release
```

if a verification fails to publish, the release already exists, so don't run
`bin/release` again, which would rebuild `target/dist`. publish the rest from it
with `hasheus publish`. to only build every platform, in `target/dist`:

```bash
bin/release --dry-run
```

once the release is complete, merge the `development` branch into the `main`
branch, which should always reflect the latest release:

```bash
git switch main
git merge development
git push origin main
```
