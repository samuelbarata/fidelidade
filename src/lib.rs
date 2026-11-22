//! Unofficial, **read-only** client for the customer area of Fidelidade, the Portuguese
//! insurer (<https://www.my.fidelidade.pt>).
//!
//! Fidelidade has no public API, and it does not email the documents (invoices,
//! premium notices, policy conditions): it only emails that a new one is available.
//! This crate logs in *as the user* and drives the same pages the website does, so a
//! program can list the policies and fetch the documents.
//!
//! ```no_run
//! use fidelidade::{Authenticator, Credentials};
//!
//! # async fn run() -> fidelidade::Result<()> {
//! let credentials = Credentials::new("123456789", "password")?;
//! let client = Authenticator::new().login(&credentials).await?;
//!
//! // One entry per insurance...
//! for policy in client.policies().await? {
//!     println!("{} ({})", policy.name, policy.number);
//! }
//!
//! // ...and the documents, of all of them or of one.
//! for document in client.documents(None).await? {
//!     let file = client.download(&document).await?;
//!     println!("{} {} {} bytes", document.date, document.kind, file.bytes.len());
//! }
//! # Ok(()) }
//! ```
//!
//! # Keeping the session
//!
//! A login is the slow, fragile part. [`Client::session`] gives a [`SessionState`] that can
//! be stored (it holds live cookies: encrypt it at rest) and resumed with
//! [`Client::from_session`]. [`Error::SessionExpired`] says when to log in again.
//!
//! ```no_run
//! use fidelidade::{Client, Config, SessionState};
//!
//! # async fn run(stored: &str) -> fidelidade::Result<()> {
//! let state: SessionState = serde_json::from_str(stored)?;
//! let client = Client::from_session(state, Config::default())?;
//! if !client.is_authenticated().await? {
//!     // log in again
//! }
//! # Ok(()) }
//! ```
//!
//! # Never retry authentication failures
//!
//! [`Error::is_auth_failure`] is `true` when the credentials were rejected. Retrying that
//! automatically can lock the account. [`Error::needs_login`] marks the cases where logging
//! in again is the right answer.

mod auth;
mod client;
mod config;
mod error;
mod html;
mod model;
mod pages;
mod session;

pub use auth::{Authenticator, Credentials};
pub use client::Client;
pub use config::Config;
pub use error::{Error, Result};
pub use model::{Document, Download, Policy};
pub use session::{CookieJarState, SessionState};
