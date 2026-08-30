//! Host-CLI commands: account recovery that runs from the machine's shell and nowhere else.
//!
//! A self-hosted single-user install has no second factor and no mailbox to send a reset link
//! to — there is no channel the server can trust more than the password it just lost. So the
//! recovery path is not a route at all: it is an argv branch that connects to the database,
//! does its work and exits before any listener is built. Nothing here is reachable over HTTP,
//! over MCP, or by an authenticated session, which is the entire security argument — the only
//! credential is being able to run a process next to the database, and anyone who can already
//! holds `DATABASE_URL` and `OTW_SECRET_KEY`. The command grants no privilege they lacked; it
//! just saves them from hand-writing an argon2 hash into `users`.
//!
//! Two rules shape the rest:
//!
//! * **The password never rides in argv.** `ps`, the shell history file and `docker inspect`
//!   all publish a process's arguments to every user on the host, and a container's command
//!   line survives in the daemon's state long after the process is gone. So the command takes
//!   a *username* and either mints a password itself (printed to that terminal only) or reads
//!   one from stdin.
//! * **What it sets is a temporary password, not the account's new credential.** It is marked
//!   `must_change_password`, so the value that crossed a terminal buys exactly one sign-in
//!   before the app demands a real one, and every existing session is revoked so a stolen
//!   cookie cannot outlive the reset.
//!
//! Encrypted data is unaffected: the vault and every stored provider secret are sealed with
//! `OTW_SECRET_KEY`, never with the login password, so a reset loses nothing.

use anyhow::Context;
use sqlx::PgPool;

use crate::auth;

/// Alphabet for a generated temporary password: Crockford base32 (no I, L, O, U), because
/// this string is read off a terminal and typed into a browser. Exactly 32 symbols, so a
/// 5-bit mask picks one uniformly — no modulo bias to reason about.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Bytes of entropy in a generated password: 20 symbols × 5 bits = 100 bits.
const PASSWORD_LEN: usize = 20;

/// Minimum length accepted for an operator-supplied password (matches the setup wizard).
const MIN_PASSWORD_LEN: usize = 8;

pub enum Command {
    ResetPassword { username: String, from_stdin: bool },
    ListUsers,
}

/// Read a command off argv, or `None` when this is an ordinary server start.
///
/// Unrecognised arguments deliberately fall through to `None` (the server boots as before):
/// `--seed-demo` is parsed elsewhere, and a typo must never leave a deployment silently
/// refusing to start.
pub fn from_args() -> Option<Command> {
    let mut args = std::env::args().skip(1);
    match args.next()?.as_str() {
        "reset-password" => {
            let mut username: Option<String> = None;
            let mut from_stdin = false;
            for arg in args {
                match arg.as_str() {
                    "--stdin" => from_stdin = true,
                    other if other.starts_with('-') => usage(&format!("unknown option `{other}`")),
                    other if username.is_some() => usage(&format!("unexpected argument `{other}`")),
                    other => username = Some(other.to_string()),
                }
            }
            match username {
                Some(username) => Some(Command::ResetPassword { username, from_stdin }),
                None => usage("reset-password needs a username"),
            }
        }
        "list-users" => Some(Command::ListUsers),
        _ => None,
    }
}

/// Print usage and exit. Never returns.
fn usage(problem: &str) -> ! {
    eprintln!("otw-core: {problem}\n");
    eprintln!("Account recovery (run on the host, e.g. `docker exec -it opentraderworld-core-1 /app/otw-core …`):");
    eprintln!("  reset-password <username>           set a printed one-time password");
    eprintln!("  reset-password <username> --stdin   read the new password from stdin instead");
    eprintln!("  list-users                          print the account names");
    std::process::exit(2);
}

/// Run a command against the database, then return so `main` can exit.
pub async fn run(cmd: Command, database_url: &str) -> anyhow::Result<()> {
    let pool = otw_store::connect_and_migrate(database_url).await?;
    match cmd {
        Command::ResetPassword { username, from_stdin } => {
            reset_password(&pool, &username, from_stdin).await
        }
        Command::ListUsers => {
            for name in otw_store::list_usernames(&pool).await? {
                println!("{name}");
            }
            Ok(())
        }
    }
}

async fn reset_password(pool: &PgPool, username: &str, from_stdin: bool) -> anyhow::Result<()> {
    let username = username.trim();
    let Some(user) = otw_store::find_user_by_username(pool, username).await? else {
        // Listing the accounts on a miss is not a disclosure here: the caller already reads
        // the whole database. It saves the round trip when the forgotten thing is the
        // username rather than the password.
        let known = otw_store::list_usernames(pool).await?;
        let known = if known.is_empty() { "none".to_string() } else { known.join(", ") };
        anyhow::bail!("no account named `{username}` (accounts: {known})");
    };

    let (password, generated) = if from_stdin {
        let mut buf = String::new();
        std::io::stdin()
            .read_line(&mut buf)
            .context("reading the new password from stdin")?;
        let password = buf.trim_end_matches(['\r', '\n']).to_string();
        if password.chars().count() < MIN_PASSWORD_LEN {
            anyhow::bail!("password must be at least {MIN_PASSWORD_LEN} characters");
        }
        (password, false)
    } else {
        (generate_password()?, true)
    };

    let hash = auth::hash_password(&password)?;
    otw_store::reset_password(pool, user.id, &hash).await?;

    // Audit trail, written straight to `app_logs` rather than through `tracing`: the layer
    // that persists tracing events is asynchronous and this process exits immediately after.
    // The row records that a reset happened, never what it set.
    if let Err(e) = otw_store::logs::insert(
        pool,
        "warn",
        "otw_core::cli",
        &format!(
            "password reset from the host CLI for account '{}'; all sessions revoked",
            user.username
        ),
    )
    .await
    {
        eprintln!("warning: the reset succeeded but could not be written to the app log: {e:#}");
    }

    println!("Password reset for '{}'.", user.username);
    if generated {
        println!("\n  Temporary password:  {password}\n");
    }
    println!("Sign in with it once; the app then asks for a password of your own.");
    println!("Every existing session was revoked.");
    Ok(())
}

/// A fresh temporary password, grouped in fives so it can be read off a terminal without
/// losing your place.
fn generate_password() -> anyhow::Result<String> {
    let mut bytes = [0u8; PASSWORD_LEN];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("getrandom: {e}"))?;
    let mut out = String::with_capacity(PASSWORD_LEN + PASSWORD_LEN / 5);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && i % 5 == 0 {
            out.push('-');
        }
        out.push(ALPHABET[(b & 31) as usize] as char);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_passwords_are_grouped_and_unambiguous() {
        let pw = generate_password().unwrap();
        assert_eq!(pw.len(), PASSWORD_LEN + 3);
        assert_eq!(pw.matches('-').count(), 3);
        assert!(pw
            .chars()
            .filter(|c| *c != '-')
            .all(|c| ALPHABET.contains(&(c as u8))));
    }

    #[test]
    fn generated_passwords_differ() {
        assert_ne!(generate_password().unwrap(), generate_password().unwrap());
    }
}
