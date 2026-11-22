# fidelidade

Unofficial, **read-only** Rust client for the customer area of
[Fidelidade](https://www.fidelidade.pt), the Portuguese insurer
(<https://www.my.fidelidade.pt>). It logs in *as the user* (NIF + password) and drives
the pages the website uses, so a program can list the policies and fetch their documents:
invoices, premium notices, policy conditions.

Fidelidade has no public consumer API, and it does not email the documents: it only
emails that a new one is available. Everything here is reverse-engineered and can break
whenever they change the site.

> **Unofficial.** This project is not affiliated with, endorsed by or supported by
> Fidelidade. It automates the website you could use by hand, with your own credentials.
> Check the portal's terms of use for your situation and use it at your own risk.

```toml
[dependencies]
fidelidade = "0.1"
```

Requires Rust 1.88 or newer.

## Usage

```rust
use fidelidade::{Authenticator, Credentials};

let credentials = Credentials::new("123456789", "password")?;
let client = Authenticator::new().login(&credentials).await?;

// One entry per insurance...
for policy in client.policies().await? {
    println!("{} ({})", policy.name, policy.number);
}

// ...and the documents, of all of them or of one.
for document in client.documents(None).await? {
    let file = client.download(&document).await?;
    println!("{} {} {} bytes", document.date, document.kind, file.bytes.len());
}
```

| | Method |
|---|---|
| Log in | `Authenticator::login` |
| Persist / restore the session | `Client::{session, from_session}` |
| Session alive? | `Client::is_authenticated` |
| The user's insurances | `Client::policies` |
| Documents, of all insurances or one | `Client::documents(Option<&Policy>)` |
| The file | `Client::download(&Document)` (`Download::is_pdf`) |

`Document::id()` is a stable key derived from policy, date, kind and the position among
identical entries (`0000000001:2026-11-16:fatura-recibo:0`): the portal gives documents
no id of its own. It is the key to store for "have I already fetched this?", and the
content hash is the airtight one. Documents that belong to no policy (capitalisation
statements, letters) have `policy_number: None` and only show up in the "all" view.

A login is the slow, fragile part: store `Client::session()` (encrypted: it holds live
cookies) and resume it with `Client::from_session` until `Error::SessionExpired`.

## Try it against the real portal

Copy `.env.example` to `.env` and fill in your NIF and password (it is git-ignored), then:

```bash
cargo run --example fidelidade -- login
```

```bash
cargo run --example fidelidade -- policies
```

```bash
cargo run --example fidelidade -- list --json
```

```bash
cargo run --example fidelidade -- download <DOCUMENT_ID> ./out
```

The session is saved to `.fidelidade-session.json` (mode 600, git-ignored) and reused, so
the portal is only logged in to when needed. `RUST_LOG=fidelidade=debug` logs every
request (URLs and statuses only).

## How it works

The customer area is a classic server-rendered OutSystems application: one big form per
page, with the page's state in the hidden `__OSVSTATE` field, and "clicking" something is
posting that form back with `__EVENTTARGET` set to the control's name. The crate replays
exactly that, as plain requests:

1. `GET /CAND_AreaCliente/AC_Documentos.aspx` redirects to the single sign-on
   (`autenticacao.fidelidade.pt`, OAuth2 code + PKCE, all parameters supplied by the
   portal); the login form is posted back and the redirect chain lands in the customer
   area.
2. "Os Meus Documentos" lists every document and has a filter per insurance.
3. "Ver documento" is one form post whose response is the PDF.

## Status

Verified against the live portal (November 2026, a real account without a second
factor): login, listing policies, listing documents (all and per policy) and downloading
a document.

Not exercised live: wrong-password pages (covered by mock tests only, with a heuristic
for the portal's message), sessions that expire mid-run, and accounts that need more
than NIF + password to log in (those stop with `Error::InteractionRequired`). The listing
showed no pagination for 25 documents; an account with far more may need the per-policy
views. If something breaks, run with `RUST_LOG=fidelidade=debug` and look for
`UnexpectedResponse`.

## Careful with the account

* A failed login can count towards a lockout. `Error::is_auth_failure()` marks errors
  that must **never** be retried automatically.
* `SessionState` holds live session cookies, and the password opens the user's whole
  insurance record. Encrypt both at rest.
* This crate only reads. Nothing in it changes the user's account, and that is meant to
  stay true.

## Contributing

Issues and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md). Never
include real NIFs, passwords, session cookies, policy numbers or documents in issues,
tests or fixtures.

## License

[MIT-0](LICENSE) (MIT No Attribution): do whatever you like with it, commercially or
not, no attribution required. It comes with no warranty and no liability.
