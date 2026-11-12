# Publishing to crates.io

The project publishes one package, `pscon`, containing the CLI and its core,
engine, and TUI modules. Users install the `pscon` command with:

```bash
cargo install pscon --locked
```

Installation requires Rust 1.98 or newer and a C/C++ build toolchain. Cargo builds
and installs the binary. Install the extension separately from the
[JSner repository](https://github.com/vyusec/JSner).

## First release

1. Push the CLI project to `vyusec/pscon`, including
   `.github/workflows/publish.yml` and `Cargo.lock`. JSner belongs in `vyusec/JSner`.
2. Sign in to [crates.io](https://crates.io), verify your account email address in
   [profile settings](https://crates.io/settings/profile), and create an API token
   with permission to create and publish `pscon`.
3. Authenticate using `cargo login --registry crates-io`. Enter the token at
   Cargo's prompt; do not commit it or put it in a command line.
4. From a clean checkout, run:

   ```bash
   cargo test --locked
   cargo publish --package pscon --locked --registry crates-io --dry-run
   cargo publish --package pscon --locked --registry crates-io
   ```

The dry run builds the package from its archive without uploading. The package
includes Rust sources, the README, Cargo.lock, Apache-2.0 LICENSE, and NOTICE.
The MPL-2.0 extension is distributed separately. Published versions cannot be
overwritten.

After publication, check installation in a temporary prefix:

```bash
install_root="$(mktemp -d)"
cargo install pscon --version 0.1.0 --locked --root "$install_root"
"$install_root/bin/pscon" --version
```

## Automated releases

Create a GitHub Actions environment named `crates-io` and add a secret named
`CARGO_REGISTRY_TOKEN` containing a crates.io token authorized to publish `pscon`.
Configure any environment protection rules your project needs.

For each subsequent release:

1. Update `[package].version` in `Cargo.toml`.
2. Run `cargo check` to update `Cargo.lock`, then commit the changes.
3. Push a tag matching the version, for example:

   ```bash
   git tag v0.1.1
   git push origin v0.1.1
   ```

The workflow checks the tag, runs tests, and verifies the archive before
publishing only `pscon`. Pull requests and manual workflow runs only verify the
package. Use either local publishing or the tag workflow for a given version.

## References

- [Cargo publishing command](https://doc.rust-lang.org/cargo/commands/cargo-publish.html)
- [Cargo packaging command](https://doc.rust-lang.org/cargo/commands/cargo-package.html)
