# AGENTS.md

## Project overview

`fidelidade` is an unofficial, **read-only** Rust client for the customer area of
Fidelidade, the Portuguese insurer (<https://www.my.fidelidade.pt>). It logs in *as the
user* (NIF + password), lists their policies and documents, and downloads the files.
Fidelidade has no public API, so everything is reverse-engineered and can break when the
site changes. It is the library half of a pair: the application that uploads the
documents to paperless-ngx lives elsewhere (`fidelidade-paperless`) and this crate must
not know about it.

Rust 2024 edition, MSRV 1.88 (`rust-version` in `Cargo.toml`, enforced in CI), async with
`reqwest`, HTML parsing with `scraper`/`regex`, cookies with `cookie_store`.

## Commands

```bash
cargo fmt                                       # format (checked in CI)
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked                             # unit + wiremock flow tests + doctests
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
cargo audit                                     # RustSec advisories (CI runs it)
cargo publish --dry-run --locked                # what would be uploaded (CI runs it)
```

CI runs all of these. The tests never reach the real portal. The example does, so only
run it with your own account (see below).

## Structure

```
src/
  lib.rs       Crate docs (compile-checked examples) and the public re-exports
  auth.rs      Authenticator, Credentials: the single sign-on login
  client.rs    Client: policies, documents, download, session persistence
  pages.rs     Parsers for "Os Meus Documentos" (policy dropdown, document rows)
  home.rs      Home page cards (renewal, agent, links) and matching them to policies
  policy.rs    "Dados gerais" and the insured units / coverages
  receipts.rs  "Recibos a pagamento" list and detail
  parse.rs     Money, dates and the portal's label/value rows
  model.rs     Policy, Document (+ id()), Download, the internal postback Handle
  html.rs      Form replay (OutSystems postbacks), regex helpers for the page scripts
  session.rs   Cookie jar, SessionState and the HTTP wrapper around reqwest
  config.rs    Config (base URL, user agent, timeout): tests point it at wiremock
  error.rs     Error (non_exhaustive), is_auth_failure(), needs_login()
tests/flow.rs        End-to-end flows against two wiremock servers (area + login host)
tests/fixtures/      Synthetic pages in the shapes the parsers match (no real data)
examples/fidelidade.rs   CLI: login / policies / list / download (real portal)
scripts/set-version.sh   Bumps Cargo.toml and Cargo.lock for a release (used by CI)
```

## How the portal works

OutSystems "traditional web": one `<form id="WebForm1">` per page; server state in the
hidden `__OSVSTATE`; a click is that form posted back with `__EVENTTARGET` = the control's
unique name. The pages' own JS does that as AJAX (`OsAjax`, answers wrapped in
`OsJSONUpdate(...)`), but the very same postback also works as a **plain** request and
answers with the whole page, which is what this crate uses (never parse `OsJSONUpdate`).
Rows and filters are addressed by position in that state (`...$ctl02$...`), not by id, so
a `Handle` (form + target) is tied to the listing that produced it.

## Rules that matter

- **Read-only, forever.** Do not add anything that changes the user's account, policies,
  payments or contact data. If a page offers an action, it is out of scope.
- **Never use or commit real data.** No real NIFs, passwords, session cookies, policy
  numbers, names, addresses or documents: not in code, tests, fixtures, issues or PRs.
  Placeholders: `123456789`, `0000000001`, `MR00000002`. Fixtures are synthetic and trimmed
  to what the parser reads; keep new ones that way. Downloaded documents are never test
  fixtures.
- **Never read or expose bank or identity details.** The IBAN, the direct-debit authorization
  number and the policyholder's details appear on the policy and receipt pages; the parsers pick
  fields by label and must never grow a generic "all rows" output for those pages (the insured
  unit's `info` rows are the one deliberate exception: they hold no bank data). Tests assert
  that nothing of the sort reaches `Debug` or JSON.
- **Never reuse the portal's guid URLs.** The server remembers the policy opened last for the
  whole session, so a stored `PolicyDetail.aspx?Guid=...` stops working (it answers with the
  error page). Walk the postback chain from the home page every time.
- **An error page is an error.** The portal serves `Exceptions.ErrorPage.aspx` with a 200 and
  sometimes a documents page without its insurance filter; `Client::check` and
  `documents_page` turn both into `UnexpectedResponse`. Never let a degraded page parse into an
  empty list.
- **Never retry authentication failures.** A failed login can lock the account.
  `Error::is_auth_failure()` errors must reach the caller untouched. Don't add retry
  loops around anything in `auth.rs`, and never run the example's `login` in a loop.
- **Don't test against other people's accounts**, and don't hammer your own.
- **Secrets stay out of `Debug` and logs.** `Credentials`, `CookieJarState` and `Client`
  redact; `Handle` prints a placeholder; `tracing` output carries URLs and statuses only
  (`RUST_LOG=fidelidade=debug`). No passwords, cookies or page bodies in errors. When
  pasting debug logs anywhere, strip query strings: redirects carry OAuth codes.
- `SessionState` holds live session cookies and `Handle` holds page state. Document that
  callers must encrypt sessions at rest whenever you touch them.
- Every public item has a doc comment. Doc examples are `no_run`.
- Parsers are matched against the real portal's output but tested on synthetic pages. If
  the portal changes, look for `UnexpectedResponse`, fix the parser, and update the
  fixture with a trimmed synthetic page, not a capture.
- `Error` is `#[non_exhaustive]`; adding variants is not a breaking change.

## Example against the real portal

Credentials live in a git-ignored `.env` (`FIDELIDADE_NIF`, `FIDELIDADE_PASSWORD`; see
`.env.example`). Agents must not read that file.

```bash
cargo run --example fidelidade -- login
cargo run --example fidelidade -- list --json
```

The session is stored in `.fidelidade-session.json` (mode 600, git-ignored). Never commit
or paste it.

## Commits and releases

Versions are semver, decided by [Conventional Commits](https://www.conventionalcommits.org):
`feat!:`/`BREAKING CHANGE:` is major (minor while `0.y.z`), `feat:` is minor (patch while
`0.y.z`), `fix:`/`perf:` are patch, and `docs:`/`chore:`/`ci:`/`test:`/`refactor:` release
nothing. Pull requests are squash-merged, so the **PR title** is the commit message.

A bot keeps a **Release merge request** on GitLab with the changelog and the next version
(`Cargo.toml` and `Cargo.lock` bumped in the same commit); merging it publishes to
crates.io. **Do not** edit `RELEASE-NOTES.md`, the `version` in `Cargo.toml`, or the
`fidelidade` entry in `Cargo.lock` by hand, and do not create `v*` tags.

## Repository and CI

Same setup as the `efatura` crate: GitLab is the source of truth, mirrored to GitHub, with
merged GitHub pull requests pushed back (fast-forward only). `.gitlab-ci.yml` runs fmt,
clippy, tests, docs, audit and a publish dry-run, mirrors master to GitHub and publishes
to crates.io when a release lands; the mirror and publish jobs only exist once their CI
variables (`GITHUB_DEPLOY_KEY`, `CARGO_REGISTRY_TOKEN`) are set. Workflow files under
`.github/` can't be changed from a GitLab commit (the deploy key cannot push them), so
change those through GitHub pull requests.
