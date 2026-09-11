# getignore

`gi` fetches a `.gitignore` template from the [github/gitignore](https://github.com/github/gitignore)
repository and writes it to a file. The template index and the templates themselves are cached, so
repeat runs are fast and work offline. Caching is lazy: `gi` caches a template the first time it
fetches that template.

## Install

From <https://crates.io>:

```sh
cargo install getignore-rs
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
gi --list                       # prints the path of every available template
gi --help
gi --version
```

If the destination already exists, `gi` shows the template it found and asks before it overwrites
the file. Enter `y` to overwrite. Enter `n`, or press Enter, to leave the file untouched. Any other
answer repeats the question.

## Matching

The language argument does not have to be the exact template name. Four tiers are tried in order,
stopping at the first that answers:

1. **Exact** match, case-insensitively and ignoring a `.gitignore` suffix, against the template
   name or any trailing part of its path. For example, `atmelstudio`, `embedded/AtmelStudio` and
   `community/embedded/AtmelStudio.gitignore` all match `community/embedded/AtmelStudio.gitignore`.
2. **Alias** from the table in `src/aliases.txt` (`js` → `Node`, `py` → `Python`, `php` →
   `Composer`, and so on).
3. **Substring**: templates whose name contains the query, case-insensitively.
4. **Fuzzy**: near misses by edit distance.

If a tier finds more than one template, `gi` does not choose between them. For example, `coldbox`
matches both `community/BoxLang/ColdBox.gitignore` and `community/CFML/ColdBox.gitignore`. `gi`
then lists the closest matches as "did you mean" suggestions and exits non-zero. A longer part of
the path, such as `boxlang/coldbox`, selects one template. Fuzzy matches are always reported as
suggestions, because `gi` never autocorrects.

## Caching

The cache lives in `~/.cache/getignore` (XDG on Linux and macOS, `%LOCALAPPDATA%` on Windows):

- `index.json` — the template index, plus the commit it was built from and when it was fetched.
  It is re-fetched after seven days. If that refresh fails, the stale index is used rather than
  failing the run.
- `files/<blob sha>` — one file per template, named by its blob SHA. A cached template is valid
  exactly when its SHA matches the current index entry, so no freshness check is needed. If `gi`
  cannot write a template to the cache, it stops without writing the destination file.

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
