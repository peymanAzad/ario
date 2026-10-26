# Cargo package contents

The `include` list in the root `Cargo.toml` controls the files shipped to
crates.io. It allows:

- `Cargo.toml`, `Cargo.lock`, `README.md`, and `LICENSE`.
- Rust sources under `common/src`, `server/src`, and `tui/src`, including tests.
- `server/schema.sql`, which the daemon embeds at compile time.

Cargo normalizes `Cargo.toml`, saves the original as `Cargo.toml.orig`, and adds
Git revision metadata when available.

The archive excludes demo recordings, the GIF, development documentation, CI
workflows, build outputs, local configuration, databases, logs, and aria2 session
files. The GIF remains in the GitHub repository; the README uses a hosted URL so
the demo does not need to be included in the Cargo archive.

If code gains another embedded asset or build input, add that file explicitly to
the `include` list. Keep test fixtures required by the packaged tests as well.

## Inspect and verify the archive

Run these commands from the repository root before publishing:

```sh
cargo package --list --locked
cargo package --locked
```

The first command lists the upload contents. The second creates the `.crate`
archive under `target/package`, extracts it, and verifies that the extracted
source builds. Neither command publishes anything.

For a local check while changes are uncommitted, append `--allow-dirty`. Release
verification should use the committed state without this flag.

## Test and install the extracted source

After `cargo package`, use the extracted package to check that tests and both
executables work without the rest of the checkout. For version `0.1.0`:

```sh
cargo test --locked --all-targets \
  --manifest-path target/package/ario-0.1.0/Cargo.toml

install_dir=$(mktemp -d)
cargo install --locked --path target/package/ario-0.1.0 --root "$install_dir"
ls "$install_dir/bin"
```

The installed directory must contain both `ario` and `ario_daemon`. Update the
version in these paths when preparing a new release.
