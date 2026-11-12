//! The real server on a port of this machine, for trying a client by hand.
//!
//! It is `dirt-api` as the tests run it: the file storage from
//! `compose.yaml`, and the stand-in identity provider in place of Clerk.
//! So the `dirt` program, the desktop app or an emulator can be signed in
//! and synced with no account anywhere and nothing sent off this machine.
//!
//! ```text
//! docker compose up -d
//! DATABASE_URL=postgres://postgres:dirt@127.0.0.1:55432/dirt_by_hand \
//!     cargo run -p dirt-sim --example serve
//! ```
//!
//! The database has to exist and is brought up to the current schema. Two
//! addresses are printed, on `PORT` and the port after it, or on any two
//! free ports. Point a client at one with `DIRT_API_BASE_URL`, and use
//! another `DIRT_DATA_DIR` than your own.
//!
//! **Signing in.** There is no sign-in page. `dirt auth login --no-browser`
//! prints an address; fetch it with `&as=<any name>` added, and that name
//! is who signed in:
//!
//! ```text
//! curl -sL "<the address>&as=alice"
//! ```
//!
//! **Two devices on one computer** use the two addresses, one each, and a
//! data folder each. The keychain keeps one device's keys for each server
//! address, port included. (`localhost` will not do for the second: the
//! clients read it as `127.0.0.1`, so that the desktop app and the CLI
//! always name one server.) Remove both keychain entries afterwards; they
//! are filed under `dev.dirt.device`.
//!
//! This is for a developer's machine. Anyone who can reach the port can
//! sign in as anyone.

use std::net::SocketAddr;

use dirt_sim::{file_bucket, Server};
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database = std::env::var("DATABASE_URL")
        .map_err(|_| "set DATABASE_URL to a database made for this, as the comment above shows")?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database)
        .await?;
    dirt_api::MIGRATOR.run(&pool).await?;
    file_bucket().await;

    let server = Server::with_sign_in(pool).await;
    let port: Option<u16> = std::env::var("PORT")
        .ok()
        .and_then(|port| port.parse().ok());
    // This machine only: see the last paragraph above.
    let bind = |port: u16| tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)));
    let first = bind(port.unwrap_or(0)).await?;
    let second = bind(port.map_or(0, |port| port.saturating_add(1))).await?;
    println!("listening on http://{}", first.local_addr()?);
    println!(
        "and, for a second device, on http://{}",
        second.local_addr()?
    );
    println!("signing in is at {}", server.issuer());
    let router = server.router();
    tokio::spawn(async move {
        let _ = axum::serve(second, router).await;
    });
    axum::serve(first, server.router()).await?;
    Ok(())
}
