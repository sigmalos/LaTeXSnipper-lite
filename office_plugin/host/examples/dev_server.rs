//! Serve the add-in with the real services on a random port and a throwaway
//! certificate, for browser tests and development. Prints the port.
//!
//!     cargo run --example dev_server [port]
//!
//! Settings go to `LATEXSNIPPER_OFFICE_STATE` (a temporary directory if unset).
//! Nothing is installed: no trust setting, LaunchAgent or manifest.

use office_host::cert::LocalCertificate;
use office_host::install::{build_host, Env};
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    let port: u16 = std::env::args()
        .nth(1)
        .map(|p| p.parse())
        .transpose()?
        .unwrap_or(0);
    let temp = tempfile::tempdir()?;
    if std::env::var_os("LATEXSNIPPER_OFFICE_STATE").is_none() {
        std::env::set_var("LATEXSNIPPER_OFFICE_STATE", temp.path());
    }
    let env = Env::current()?;
    let host = Arc::new(build_host(&env)?);
    let identity = LocalCertificate::in_dir(&temp.path().join("cert"));
    identity.generate()?;
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    let port = listener.local_addr()?.port();
    office_host::http::spawn(listener, identity.tls_config()?, port, host);
    println!("{port}");
    loop {
        std::thread::park();
    }
}
