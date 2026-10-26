# Contributing

Thanks for helping with videode. Bug reports, ideas and pull requests are all welcome.

## Before you start

For anything bigger than a bug fix, open an issue first and describe what you'd change. videode follows a written plan ([`docs/PLAN.md`](docs/PLAN.md)) and design system ([`DESIGN.md`](DESIGN.md)), and agreeing on the approach early saves rework.

## Building

See the Build section of the [README](README.md) for each platform's dependencies, including FFmpeg. Then:

```sh
cargo run -p videode-app
```

The demo project (File → Open demo project) generates its footage with the `ffmpeg` tool on first open, so it's a quick way to try changes with real media.

## Before opening a pull request

Run the same checks as CI:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

For UI changes, attach a screenshot. The snapshot example renders the window offscreen:

```sh
cargo run -p videode-app --example snapshot -- out.png 1440 900 demo
```

## Conventions

- **Commits** follow [Conventional Commits](https://www.conventionalcommits.org/): `feat(media): …`, `fix: …`, `docs: …`, `ci: …`.
- **Everything is a node.** New effects and operations belong in the node graph (`videode-core`), with the editor UI on top.
- **Project files are KDL**, not JSON. Keep `.vde` files readable by hand.
- **No UI in `videode-core`.** It stays testable without a window.
- **Tests:** core logic gets unit tests; UI interactions go in `crates/videode-app/src/tests.rs`.
- Respect the reduce-motion setting for any new animation.

## License

By contributing, you agree that your contributions are licensed under the [GNU LGPL v3.0 or later](COPYING.LESSER), the same as the rest of videode.
