//! `cf.exe` in the user's terminal on Windows: the program `cf setup` and the
//! app's repair copy to `cf.exe` and `consensflow.exe` in the home's `bin`,
//! which runs the app's own `cf` with every argument as it was given
//! (`cf_launcher::shim`). A `.cmd` in its place cut an argument at its first
//! line break.
//!
//! It is a program of its own, and not the `cf`: it is small and it is the same
//! in every release, so an update of the app never has to replace the copy a
//! terminal is running.

#![forbid(unsafe_code)]

fn main() {
    std::process::exit(cf_launcher::shim::run());
}
