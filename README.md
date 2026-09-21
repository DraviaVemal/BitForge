# BitForge

A single-binary Yocto / BitBake workspace manager with declarative layers, a
lockfile and a live build web UI.

## Purpose

`BitForge` makes Yocto builds reproducible and easy to manage from one static
binary — no Python virtualenv or global installs required. It:

- Scaffolds a Yocto workspace and clones poky / OpenEmbedded-core plus BitBake
  with a single `BitForge --init`.
- Manages upstream layers declaratively in `BitForge.toml`, keeping the manifest
  and `bblayers.conf` in sync automatically.
- Pins every layer to an exact commit in `BitForge.lock`, so a fresh checkout
  rebuilds the same tree.
- Generates your BitBake configuration and serves a built-in web UI that streams
  BitBake output live — start and watch builds from the CLI or the browser.
- Updates itself in place with `BitForge --update` (with a `--beta` channel for
  early builds).

## Installation

Download and install the latest stable binary with the install script:

```sh
curl -fsSL https://raw.githubusercontent.com/DraviaVemal/BitForge/release/install.sh | sh
```

This installs the `BitForge` binary to `~/.local/bin`. Options:

```sh
# Install the latest pre-release (beta) build
curl -fsSL https://raw.githubusercontent.com/DraviaVemal/BitForge/release/install.sh | sh -s -- --beta

# Install to a custom directory
curl -fsSL https://raw.githubusercontent.com/DraviaVemal/BitForge/release/install.sh | sh -s -- --dir=/usr/local/bin
```

Make sure the install directory is on your `PATH`, then run `BitForge --init` to
bootstrap your first workspace.

## Documentation

Full documentation — getting started, layer and workspace guides, the build web
UI, the complete `BitForge.toml` schema and the CLI reference — lives at:

**https://docs.draviavemal.com/bitforge**

## License

`BitForge` is **dual-licensed** — use it under whichever fits your case:

1. **GNU GPL-3.0** (open source) — the full text is in the [`LICENSE`](LICENSE)
   file. You may use, study, modify and distribute the software, including for
   commercial purposes, provided distributed derivative works remain open under
   GPL-3.0.
2. **Commercial license** — for closed-source / proprietary use without the
   GPL-3.0 copyleft obligations, offered through
   [GitHub Sponsors](https://github.com/sponsors/DraviaVemal). It adds
   commercial-friendly (MIT-style) terms, access to a private packaged-release
   repository and priority support.

See the [licensing overview](https://docs.draviavemal.com/bitforge/license/) for
details, and [Versioning & Licensing](https://docs.draviavemal.com/versioning#licensing)
for how terms can differ between versions. For custom arrangements, contact
**contact@draviavemal.com**.

## Contributing

Contributions target the single `release` branch:

1. Fork the repository and create a feature branch from `release`.
2. Make your change in clear, focused commits using Conventional Commits
   (e.g. `feat: add layer prune command`, `fix: handle missing lockfile`).
3. Add or update tests and docs where relevant.
4. Open a pull request against `release` describing the change and its
   motivation. Merging publishes a new alpha automatically; stable releases are
   cut later by a maintainer pushing a `vX.Y.Z` tag.

A good bug report includes the version (stable or alpha), a minimal reproducible
example, expected vs. actual behaviour, and your OS and toolchain versions.
Please keep interactions respectful and constructive. See the shared
[community & contributing guide](https://docs.draviavemal.com/community) for more.
