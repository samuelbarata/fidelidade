//! Small CLI to exercise the crate against the real portal.
//!
//! ```text
//! cargo run --example fidelidade -- login
//! cargo run --example fidelidade -- policies
//! cargo run --example fidelidade -- list [POLICY_NUMBER] [--json]
//! cargo run --example fidelidade -- download <DOCUMENT_ID> [DIR]
//! cargo run --example fidelidade -- receipts [--json]
//! cargo run --example fidelidade -- receipt <RECEIPT_NUMBER> [--json]
//! cargo run --example fidelidade -- detail <POLICY_NUMBER> [--json]
//! cargo run --example fidelidade -- coverages <POLICY_NUMBER> [--json]
//! ```
//!
//! Credentials come from `FIDELIDADE_NIF` / `FIDELIDADE_PASSWORD`, read from the
//! environment or from a git-ignored `.env` file (see `.env.example`); anything missing
//! is asked for. The session is kept in `$FIDELIDADE_SESSION` (default
//! `.fidelidade-session.json`) and reused, so the portal is only logged in to when that
//! session has expired. Every command is read-only.
//!
//! A failed login counts towards a possible account lockout: only use your own account,
//! and don't loop. Set `RUST_LOG=fidelidade=debug` to see every request.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use fidelidade::{Authenticator, Client, Config, Credentials, SessionState};

type BoxError = Box<dyn std::error::Error>;

fn session_path() -> PathBuf {
    std::env::var_os("FIDELIDADE_SESSION")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".fidelidade-session.json"))
}

fn prompt(label: &str) -> io::Result<String> {
    print!("{label}");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_owned())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), BoxError> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn save(client: &Client) -> Result<(), BoxError> {
    let path = session_path();
    write_private(&path, &serde_json::to_vec_pretty(&client.session()?)?)?;
    eprintln!("session saved to {}", path.display());
    Ok(())
}

/// Log in with the credentials from the environment (or prompts) and save the session.
async fn login() -> Result<Client, BoxError> {
    let nif = match std::env::var("FIDELIDADE_NIF")
        .ok()
        .filter(|v| !v.is_empty())
    {
        Some(nif) => nif,
        None => prompt("NIF: ")?,
    };
    let password = match std::env::var("FIDELIDADE_PASSWORD")
        .ok()
        .filter(|v| !v.is_empty())
    {
        Some(password) => password,
        None => rpassword::prompt_password("Password: ")?,
    };
    let credentials = Credentials::new(nif, password)?;
    let client = Authenticator::new().login(&credentials).await?;
    save(&client)?;
    Ok(client)
}

async fn find_policy(client: &Client, number: &str) -> Result<fidelidade::Policy, BoxError> {
    client
        .policies()
        .await?
        .into_iter()
        .find(|p| p.number == number)
        .ok_or_else(|| format!("no policy {number}").into())
}

/// Reuse the saved session if it is still alive, otherwise log in.
async fn connect() -> Result<Client, BoxError> {
    if let Ok(raw) = std::fs::read(session_path()) {
        let state: SessionState = serde_json::from_slice(&raw)?;
        let client = Client::from_session(state, Config::default())?;
        if client.is_authenticated().await? {
            // The portal renews a stale session through the persistent-login cookie and
            // rotates cookies as it goes: keep the fresh ones, or the next run starts from
            // the old set again.
            write_private(
                &session_path(),
                &serde_json::to_vec_pretty(&client.session()?)?,
            )?;
            return Ok(client);
        }
        eprintln!("saved session expired, logging in again");
    }
    login().await
}

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(io::stderr)
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let args: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| *a != "--json")
        .collect();

    match args.as_slice() {
        ["login"] => {
            let client = login().await?;
            eprintln!("logged in as NIF {}", client.nif());
        }
        ["policies"] => {
            let policies = connect().await?.policies_with_details().await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&policies)?);
            } else {
                for p in policies {
                    println!(
                        "{:<12} {:<8} {}",
                        p.number,
                        p.category.unwrap_or_default(),
                        p.name
                    );
                }
            }
        }
        ["list", rest @ ..] => {
            let client = connect().await?;
            let policy = match rest {
                [] => None,
                [number] => Some(
                    client
                        .policies()
                        .await?
                        .into_iter()
                        .find(|p| p.number == *number)
                        .ok_or_else(|| format!("no policy {number}"))?,
                ),
                _ => return Err("usage: list [POLICY_NUMBER] [--json]".into()),
            };
            let docs = client.documents(policy.as_ref()).await?;
            if json {
                let rows: Vec<_> = docs
                    .iter()
                    .map(|d| {
                        let mut v = serde_json::to_value(d).expect("serializable");
                        v["id"] = d.id().into();
                        v
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                for d in docs {
                    println!("{}  {}", d.date, d.kind);
                    println!("            {}", d.id());
                }
            }
        }
        ["download", id, rest @ ..] => {
            let dir = rest
                .first()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            let client = connect().await?;
            let docs = client.documents(None).await?;
            let doc = docs
                .iter()
                .find(|d| d.id() == *id)
                .ok_or_else(|| format!("no document {id} (run `list` for the ids)"))?;
            let file = client.download(doc).await?;
            std::fs::create_dir_all(&dir)?;
            // Name it ourselves: the portal's file name is the same for every invoice.
            let target = dir.join(format!("{}.pdf", id.replace(':', "_")));
            std::fs::write(&target, &file.bytes)?;
            eprintln!(
                "{} bytes ({}, portal name {:?}) -> {}",
                file.bytes.len(),
                if file.is_pdf() { "pdf" } else { "NOT a pdf" },
                file.filename,
                target.display()
            );
        }
        ["receipts"] => {
            let receipts = connect().await?.receipts_due().await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&receipts)?);
            } else if receipts.is_empty() {
                println!("no receipts waiting to be paid");
            } else {
                for r in receipts {
                    println!(
                        "{}  {:>9.2} EUR  {}  ({}, {})",
                        r.due_date,
                        r.amount_cents as f64 / 100.0,
                        r.number,
                        r.product,
                        r.payment_method
                    );
                }
            }
        }
        ["receipt", number] => {
            let client = connect().await?;
            let receipt = client
                .receipts_due()
                .await?
                .into_iter()
                .find(|r| r.number == *number)
                .ok_or_else(|| format!("no receipt {number}"))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&client.receipt(&receipt).await?)?
            );
        }
        ["detail", number] => {
            let client = connect().await?;
            let policy = find_policy(&client, number).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&client.policy_detail(&policy).await?)?
            );
        }
        ["coverages", number] => {
            let client = connect().await?;
            let policy = find_policy(&client, number).await?;
            let units = client.coverages(&policy).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&units)?);
            } else {
                for unit in units {
                    println!("{}", unit.name);
                    for (label, value) in &unit.info {
                        println!("    {label}: {value}");
                    }
                    for c in &unit.coverages {
                        println!(
                            "  - {} [{:?}] {}",
                            c.name,
                            c.capital,
                            c.deductible.as_deref().unwrap_or("")
                        );
                    }
                }
            }
        }
        _ => {
            return Err(
                "usage: login | policies | list [POLICY] | download <ID> [DIR] | receipts | receipt <N> | detail <POLICY> | coverages <POLICY>   (--json where it applies)"
                    .into(),
            );
        }
    }
    Ok(())
}
