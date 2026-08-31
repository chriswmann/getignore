# getignore

`gi` fetches a `.gitignore` template from the [github/gitignore](https://github.com/github/gitignore)
repository and writes it to a file. The template index and the templates themselves are cached, so
repeat runs are fast and work offline. Caching is lazy and best effort only: templates are cached
when requested and the request is successful.

## Install

From <https://crates.io>:

```sh
cargo install getignore
```

From the cloned repo:

```sh
cargo install --path .
```

This builds the binary as `gi`.

## Usage

```sh
gi python                       # writes ./.gitignore
gi rust -d ~/projects/foo/.gitignore
gi --help
gi --version
```

If the destination already exists, `gi` asks before overwriting; anything other than `y` leaves the
file untouched.

## Matching

The language argument does not have to be the exact template name. Four tiers are tried in order,
stopping at the first that answers:

1. **Exact** match against the template path, case-insensitively and ignoring a `.gitignore` suffix
   (`python`, `Python`, `Python.gitignore`, `community/BoxLang/ColdBox`).
2. **Alias** from the table in `src/aliases.txt` (`js` → `Node`, `py` → `Python`, `php` →
   `Composer`, and so on).
3. **Substring**: the first template whose path contains the query.
4. **Fuzzy**: near misses by edit distance. These are only ever reported as "did you mean"
   suggestions — `gi` never autocorrects, and exits non-zero instead.

## Caching

The cache lives in `~/.cache/getignore` (XDG on Linux and macOS, `%LOCALAPPDATA%` on Windows):

- `index.json` — the template index, plus the commit it was built from and when it was fetched.
  It is re-fetched after seven days. If that refresh fails, the stale index is used rather than
  failing the run.
- `files/<blob sha>` — one file per template, named by its blob SHA. A cached template is valid
  exactly when its SHA matches the current index entry, so no freshness check is needed.

Clear it with `rm -r ~/.cache/getignore`, or `mise run clear-cache`.

## Logging

Logging goes through `tracing` with an env filter, defaulting to `warn`:

```sh
RUST_LOG=debug gi python
```

## Development

```sh
cargo build
cargo run -- python
cargo test
cargo clippy -- -D warnings
cargo fmt
```

[mise](https://mise.jdx.dev) tasks refresh the test fixtures from the live repository:

```sh
mise run fetch-branch     # data/branch.json + tests/fixtures/branch-fixture.json
mise run fetch-tree       # data/trees.json + the tree fixtures
```
