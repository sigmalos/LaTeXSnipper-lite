//! `latexsnipper-office-host`, inside LaTeXSnipper Office.app.
//!
//!     --serve            the host launchd keeps running (login agent)
//!     --install          trust the certificate, register and start the agent
//!     --uninstall        remove the agent, manifests, certificate trust and state
//!     --status           JSON status for the Setup window
//!     --download-model   download the formula model; JSON lines of progress
//!     --self-test        serve the bundled files on a free port and check them
//!
//! Setup (the app's window) runs these commands; nothing here needs the
//! LaTeXSnipper desktop app.

use office_host::install::{self, Env};
use office_host::{paths, recognition};
use serde_json::json;
use std::process::ExitCode;

fn fail(error: anyhow::Error) -> ExitCode {
    // First line is a stable code (e.g. TRUST_DENIED:…) the Setup window maps to a message.
    eprintln!("{error:#}");
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    // Finder may add -psn_* arguments; anything unknown is ignored.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let env = match Env::current() {
        Ok(env) => env,
        Err(e) => return fail(e),
    };
    if has("--version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if has("--serve") {
        return match install::serve(&env) {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => {
                paths::log(format!("{e:#}"));
                ExitCode::FAILURE
            }
        };
    }
    let result = if has("--install") {
        install::install(&env)
    } else if has("--uninstall") {
        install::uninstall(&env)
    } else if has("--status") {
        Ok(install::status(&env))
    } else if has("--download-model") {
        download(&env)
    } else if has("--self-test") {
        install::self_test(&env)
    } else {
        eprintln!("usage: latexsnipper-office-host --serve | --install | --uninstall | --status | --download-model");
        return ExitCode::from(2);
    };
    match result {
        Ok(value) => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(e),
    }
}

/// Into the add-in's own folder; the desktop app's models are never written.
fn download(env: &Env) -> anyhow::Result<serde_json::Value> {
    paths::ensure_private_dir(&env.state)?;
    let root = paths::own_models_dir(&env.state);
    let mut last = 0u64;
    mathcraft::modelhub::download(&root, recognition::FORMULA_MODEL, &mut |p| {
        // About one line per percent.
        if p.total == 0 || p.downloaded >= p.total || p.downloaded >= last + p.total / 100 {
            last = p.downloaded;
            println!("{}", json!({"downloaded": p.downloaded, "total": p.total}));
        }
    })
    .map_err(|e| anyhow::anyhow!("DOWNLOAD_FAILED:{e:#}"))?;
    Ok(install::status(env))
}
