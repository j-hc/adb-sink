use adb_sink::adb::{AdbCmd, AdbErr, AdbShell};
use adb_sink::args::{Cli, PullArgs, PushArgs, SubCmds};
use adb_sink::fs::{AndroidFS, LocalFS};
use adb_sink::VERBOSE;
use adb_sink::{adb_connect, sink, Result};
use chainerror::Context;
use clap::Parser;
use std::process::ExitCode;

fn run(args: Cli) -> Result<()> {
    match AdbCmd::run_v(["start-server"]) {
        Ok(_) => {}
        Err(AdbErr::IO(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            panic!("adb binary not found")
        }
        Err(AdbErr::Adb(e)) if e.starts_with("* daemon not running") => {}
        Err(e) => panic!("{}", e),
    }
    adb_connect().annotate()?;

    let mut android_fs = AndroidFS {
        shell: AdbShell::new().annotate()?,
    };

    match args.subcmd {
        SubCmds::Pull(PullArgs {
            source,
            dest,
            delete_if_dne,
            ignore_dir,
            set_times,
        }) => {
            let dest = match dest {
                Some(dest) => dest,
                None => std::env::current_dir().expect("could not get current dir"),
            };
            sink(
                &mut android_fs,
                &mut LocalFS,
                source,
                dest,
                delete_if_dne,
                ignore_dir,
                set_times,
            )
            .annotate()?;
        }
        SubCmds::Push(PushArgs {
            source,
            dest,
            delete_if_dne,
            ignore_dir,
        }) => {
            sink(
                &mut LocalFS,
                &mut android_fs,
                source,
                dest,
                delete_if_dne,
                ignore_dir,
                false,
            )
            .annotate()?;
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let args = Cli::parse();
    VERBOSE.set(args.verbose).unwrap();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ERROR: {}", e);
            eprintln!("{:?}", e);
            ExitCode::FAILURE
        }
    }
}
