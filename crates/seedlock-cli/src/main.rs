//! SeedLock command-line front end.
//!
//! All the logic lives in `seedlock-core`; this crate only asks questions,
//! prints results and moves files around. It never uses the network.

#![deny(unsafe_code)]

mod cli;
mod commands;
mod qrimg;
mod netcheck;
mod qrwindow;
mod scan;
mod ui;

use clap::Parser;
use cli::{Cli, Cmd};
use std::process::ExitCode;

fn main() -> ExitCode {
    harden();
    #[cfg(windows)]
    ui::enable_terminal_codes();
    let cli = Cli::parse();
    let done = |r: ui::Res<()>| r.map(|_| ExitCode::SUCCESS);
    let result = match cli.command {
        None => done(commands::menu()),
        Some(Cmd::New(a)) => done(commands::new_seed(a)),
        Some(Cmd::Encrypt(a)) => done(commands::encrypt(a)),
        Some(Cmd::Decrypt(a)) => done(commands::decrypt(a)),
        Some(Cmd::Verify(a)) => commands::verify(a).map(|ok| if ok { ExitCode::SUCCESS } else { ExitCode::from(2) }),
        Some(Cmd::Info(a)) => done(commands::info(a)),
        Some(Cmd::Sign(a)) => done(commands::sign(a)),
        Some(Cmd::Inspect(a)) => done(commands::inspect(a)),
        Some(Cmd::Qr(a)) => done(commands::qr(a)),
        Some(Cmd::Camera(a)) => done(commands::camera(a)),
        Some(Cmd::Password(a)) => done(commands::password(a)),
        Some(Cmd::FinalWord) => done(commands::final_word()),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Basic process hardening: no core dumps, so a crash can never write secrets to disk.
#[cfg(unix)]
#[allow(unsafe_code)]
fn harden() {
    let zero = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: plain libc calls with valid arguments; they only lower this process's own limits.
    unsafe {
        libc::setrlimit(libc::RLIMIT_CORE, &zero);
        #[cfg(target_os = "linux")]
        libc::prctl(
            libc::PR_SET_DUMPABLE,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
        );
    }
}

#[cfg(not(unix))]
fn harden() {}
