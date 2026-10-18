//! SeedLock works only on a computer that is offline. [`require`] checks before SeedLock does anything:
//! the menu then shows only what it found and how to disconnect (Enter checks again), and a command stops.
//! [`watch`] then checks every second: if the computer goes online meanwhile, or SeedLock can no longer tell,
//! it clears the screen and stops at once, so that no seed stays open on a computer that is connected.
//!
//! There is no way around it, on purpose. In an emergency, disconnect the computer, or open the backup with
//! the independent recovery tool (tools/recover.py).

use crate::netcheck::{self, NetState};
use crate::ui;
use std::io::Write;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

/// The exit code when SeedLock does not work because the computer is not offline.
pub const NOT_OFFLINE: u8 = 3;

/// How often [`watch`] looks.
const EVERY: Duration = Duration::from_secs(1);

/// How many times in a row the state may be unknown (a `route` program that did not answer once) before
/// [`watch`] stops SeedLock. Online stops it the first time.
const UNKNOWN_TIMES: u32 = 3;

/// Checks that the computer is offline before SeedLock does anything. In the menu (`menu`, with a person
/// at the terminal), SeedLock says what it found and checks again each time Enter is pressed, until the
/// computer is offline or the person quits; a command stops at once. `Err` holds the exit code.
pub fn require(menu: bool) -> Result<(), ExitCode> {
    let ask = menu && ui::interactive();
    let mut said = false;
    loop {
        let state = netcheck::state();
        if matches!(state, NetState::Offline) {
            return Ok(());
        }
        if said {
            eprintln!("{}", still(&state));
        } else {
            explain(&state);
            said = true;
        }
        if !ask {
            return Err(ExitCode::from(NOT_OFFLINE));
        }
        match ui::read_line("\nPress Enter to check again, or type q to quit: ") {
            Ok(typed) if !typed.trim().eq_ignore_ascii_case("q") => continue,
            _ => return Err(ExitCode::from(NOT_OFFLINE)),
        }
    }
}

/// The connections found, as SeedLock names them.
fn through(interfaces: &[String]) -> String {
    interfaces.join(", ")
}

/// Why SeedLock does not work, and what to do: in a box, the first time.
fn explain(state: &NetState) {
    let title = "SEEDLOCK WORKS ONLY OFFLINE";
    match state {
        NetState::Online(interfaces) => {
            let through = through(interfaces);
            match netcheck::virtual_machine() {
                Some(vm) => {
                    let found = format!(
                        "SeedLock is running in a virtual machine ({vm}) whose network adapter is switched on (through \
                         {through}). A virtual machine's network adapter always looks connected, even while the real \
                         computer is offline, and it is online again as soon as the real computer is: from in here, \
                         SeedLock cannot see the real computer's connection."
                    );
                    ui::boxed_note(
                        title,
                        &[
                            found.as_str(),
                            "SeedLock does not work while a connection is possible: other programs could see your seed words \
                             or your password. Turn off this virtual machine's network adapter (in the virtual machine \
                             program's settings, or in this system): then SeedLock sees it as offline.",
                        ],
                    );
                }
                None => {
                    let found = format!(
                        "This computer is connected to a network (through {through}). SeedLock does not work while a \
                         connection is possible: other programs could see your seed words or your password while they are \
                         on the screen or being typed."
                    );
                    ui::boxed_note(
                        title,
                        &[
                            found.as_str(),
                            "Disconnect it: turn off Wi-Fi and unplug any network cable. Best of all, use a computer that \
                             is never connected.",
                        ],
                    );
                }
            }
        }
        NetState::Unknown(why) => {
            let found = format!(
                "SeedLock cannot tell whether this computer is online ({why}). It works only where it can see that the \
                 computer is offline."
            );
            ui::boxed_note(
                title,
                &[
                    found.as_str(),
                    "It reads the computer's own routing table, on Linux, macOS and Windows: use one of these, and not \
                     inside a sandbox or a container that hides that table.",
                ],
            );
        }
        NetState::Offline => {}
    }
}

/// One line after Enter, while nothing changed.
fn still(state: &NetState) -> String {
    match state {
        NetState::Online(interfaces) => format!("Still connected (through {}).", through(interfaces)),
        NetState::Unknown(why) => format!("SeedLock still cannot tell whether this computer is online ({why})."),
        NetState::Offline => String::new(),
    }
}

/// From now on, checks every second that the computer is still offline (see the module's description).
pub fn watch() {
    let _ = thread::Builder::new().name("offline-check".to_string()).spawn(|| {
        let mut unknown = 0;
        loop {
            thread::sleep(EVERY);
            match netcheck::state() {
                NetState::Offline => unknown = 0,
                NetState::Online(interfaces) => {
                    stop(&format!("A network connection appeared while SeedLock was open (through {}).", through(&interfaces)))
                }
                NetState::Unknown(why) => {
                    unknown += 1;
                    if unknown >= UNKNOWN_TIMES {
                        stop(&format!("SeedLock could no longer tell whether this computer is online ({why})."))
                    }
                }
            }
        }
    });
}

/// Stops SeedLock at once: the QR window and the camera's programs are ended, the terminal is put back (it
/// shows what is typed again), the screen and its scrollback are cleared, and the box says why. Ending the
/// process lets go of everything it had open.
fn stop(what: &str) -> ! {
    // While these are held, nothing else that SeedLock writes can come in between.
    let out = std::io::stdout();
    let _out = out.lock();
    let err = std::io::stderr();
    let mut err = err.lock();
    crate::children::end_all(); // the QR window and the camera's programs end at once too
    ui::restore_terminal();
    ui::clear_screen();
    ui::boxed_note(
        "SEEDLOCK STOPPED: THIS COMPUTER IS NOT OFFLINE",
        &[
            what,
            "SeedLock works only offline, so it cleared the screen and stopped at once: nothing that was open stays in \
             memory. Disconnect the computer (turn off Wi-Fi, unplug any network cable), then start SeedLock again.",
        ],
    );
    let _ = err.flush();
    std::process::exit(i32::from(NOT_OFFLINE));
}
