# agents

notes for coding agents working on hasheus-cli. the [readme](README.md) is the
runbook for setup, config, sample data, and releasing; this file covers what it
doesn't.

## checks

a change is done when all of these pass. ci runs the same, with the tests under
cargo-llvm-cov for coverage:

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
```

the tests run against nostr-sdk's in-process relay, so they need no network or
running relay.

## layout

- `src/` is flat: one module per file, each with one job and named after it. a
  module that acts is named as a doer (`verification_fetcher`,
  `file_hash_reporter`, `secret_key_reader`); one that holds data is a plain
  noun (`relay_config`, `file_event`). a new concept gets a new module, declared in
  alphabetical order in `main.rs`; anything else goes in the module it supports.
- `main.rs` and `cli.rs` stay thin. `file_verifier` and `verification_publisher`
  orchestrate `verify` and `publish`, and delegate to the other modules.
- tests live in a `#[cfg(test)] mod tests` at the bottom of the module they
  test. shared test helpers are the test-only `test_event_builders` and
  `test_relays` modules (`run_relay`, `build_relay_config`, `DEAD_RELAY_URL`).

## conventions

- **all copy is lowercase**: help text, messages, errors, comments, and docs.
  clap's own help text is rewritten to match in `cli.rs`, and the
  `lowercases_the_help` test enforces it. env vars and other case-sensitive
  tokens keep their case.
- **errors**: every failure is a variant of `HasheusError`, with a lowercase
  message in its `Display` and a stable snake_case `code()` for the json
  output. scripts branch on the code, so never rename an existing one. never
  include a secret key, password, or a value that could be a mangled secret in a
  message.
- **output**: with `--json`, results and errors both go to stdout, errors as
  `{"error":{"code":…,"message":…}}` with exit code 1. an unverified file isn't
  an error: the exit code is 0 unless something fails. text output only prompts
  with a terminal on stdin (`OutputFormat::can_prompt`), and json never does.
- **untrusted input**: anyone can publish to nostr, so only http and https urls
  are shown, and text from an event goes through `text_sanitizer::sanitize`
  before it's printed as text. json escapes it already, so `--json` passes it
  through as is.
- **parity with the web app**: the cli mirrors
  [hasheus](https://github.com/tinfoilhash/hasheus) (cloned next to this repo as
  `../hasheus`): the same event tags, form limits, caching, and verdicts. check
  the web app before changing any of those, and say so in a comment when
  matching it ("like the web page, …").
- **comments** are rare and explain why, not what, in lowercase full sentences.
- new words that trip the spell checker go in `cSpell.words` in
  [`.vscode/settings.json`](.vscode/settings.json), alphabetized.
- the rust version is pinned in [`rust-toolchain.toml`](rust-toolchain.toml) and
  `rust-version` in [`Cargo.toml`](Cargo.toml); bump both together.

## don't

- run `bin/release` without `--dry-run`: it creates a github release and
  publishes verifications to the production relays.
- run `hasheus publish` against the production relays. point
  `HASHEUS_FILE_RELAY_URLS` and `HASHEUS_USER_RELAY_URLS` at a local relay, like
  `nak serve` (see the readme's sample data), since a published verification
  can't be undone.
