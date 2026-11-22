# Contributing

Thanks for helping. This crate talks to an insurer's customer area with people's real
accounts, so a few rules matter more than usual.

## Ground rules

- **Never commit real data.** No real NIFs, passwords, session cookies, policy numbers,
  names, addresses or documents, in code, tests, fixtures, issues or PRs. Use obvious
  placeholders (`123456789`, `0000000001`, `MR00000002`).
- **Tests never reach Fidelidade.** The suite runs against `wiremock` and the HTML
  fixtures in `tests/fixtures/`. If you look at a page of the real portal, write a
  synthetic, trimmed fixture in the same shape, not a capture.
- **Read-only.** This crate lists and downloads. Pull requests that change anything in a
  user's account will not be merged.
- **Don't test against other people's accounts**, and never loop on a failed login.
- **Never retry authentication failures automatically.** `Error::is_auth_failure()` errors
  must reach the caller untouched.

## Before opening a PR

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs the same, plus `cargo doc`, `cargo audit` and a build on the minimum supported
Rust version (`rust-version` in `Cargo.toml`).

## Commit messages and releases

Versions follow [semver](https://semver.org) and are decided by commit messages, so please
write [Conventional Commits](https://www.conventionalcommits.org). When a pull request is
squash-merged its **title** becomes the commit message, so the title is the one that
matters.

| Title | Next version |
|---|---|
| `feat!: ...` or a `BREAKING CHANGE:` footer | major (minor while the crate is `0.y.z`) |
| `feat: ...` | minor (patch while `0.y.z`) |
| `fix: ...`, `perf: ...` | patch |
| `docs:`, `chore:`, `ci:`, `test:`, `refactor:`, ... | no release |

A bot keeps a **Release merge request** up to date with the changelog and the next version.
Merging it releases: the crate is published to crates.io from that merge. Don't edit
`RELEASE-NOTES.md` or the `version` in `Cargo.toml` by hand.

## License

By submitting a contribution you agree to license it under the project's
[MIT-0](LICENSE) license.

## Reporting a portal change

If Fidelidade changed their site and something broke, run with
`RUST_LOG=fidelidade=debug`, look for `UnexpectedResponse`, and describe which step failed
(without pasting cookies, query strings or personal data).
