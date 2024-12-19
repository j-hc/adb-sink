use adb_sink::adb::{AdbCmd, AdbErr, AdbShell};
use adb_sink::args::{Cli, SubCmds};
use adb_sink::fs::{AndroidFS, LocalFS};
use adb_sink::{adb_connect, sink, CResult};
use chainerror::Context;
use clap::Parser;
use std::process::ExitCode;

fn run(args: Cli) -> CResult<()> {
    match AdbCmd::run_v(["start-server"]) {
        Ok(_) => {}
        Err(AdbErr::IO(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            panic!("adb binary not found")
        }
        Err(AdbErr::Adb(e)) if e.starts_with("* daemon not running") => {}
        Err(e) => panic!("{}", e),
    }
    adb_connect().annotate()?;

    let mut local_fs = LocalFS;
    let mut android_fs = AndroidFS {
        shell: AdbShell::new().annotate()?,
    };

    {
        let p = match &args.subcmd {
            SubCmds::Pull(pa) => &pa.source,
            SubCmds::Push(pa) => &pa.source,
        };
        if !(p.starts_with("/") || p.is_absolute()) {
            return Err("Source path must be absolute".into());
        }
    }

    match args.subcmd {
        SubCmds::Pull(pa) => sink(
            &mut android_fs,
            &mut local_fs,
            pa.source,
            match pa.dest {
                Some(dest) if dest == std::path::Path::new(".") => {
                    std::env::current_dir().expect("get current dir")
                }
                Some(dest) => dest,
                None => std::env::current_dir().expect("get current dir"),
            },
            pa.delete_if_dne,
            pa.ignore_dir,
            pa.set_times,
        ),
        SubCmds::Push(pa) => sink(
            &mut local_fs,
            &mut android_fs,
            pa.source,
            pa.dest,
            pa.delete_if_dne,
            pa.ignore_dir,
            false,
        ),
    }
    .annotate()?;
    Ok(())
}

fn main() -> ExitCode {
    let args = Cli::parse();
    adb_sink::VERBOSE.set(args.verbose).unwrap();

    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ERROR: {:?}", e);
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::PathBuf, str::FromStr};

    #[test]
    fn it_works() {
        adb_sink::VERBOSE.set(true).unwrap();
        sink(
            &mut LocalFS,
            &mut LocalFS,
            PathBuf::from_str(r"test-from").unwrap(),
            PathBuf::from_str(r"test-to").unwrap(),
            true,
            Vec::new(),
            false,
        )
        .unwrap();
    }
}
