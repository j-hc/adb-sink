use adb_sync::adb::AdbErr;
use adb_sync::adb::AdbShell;
use adb_sync::adb_cmd;
use adb_sync::fs::AsStr;
use adb_sync::fs::{AndroidFS, FileSystem, LocalFS, SyncFile};
use clap::{Args, Parser, Subcommand};
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::io::Write;
use std::path::PathBuf;
use typed_path::{UnixPath, UnixPathBuf};

#[derive(Args, Debug)]
#[command(arg_required_else_help(true))]
struct PullPushArgs {
    source: PathBuf,
    dest: PathBuf,

    /// set modified time of files
    #[arg(short = 't', long)]
    set_times: bool,

    /// delete files on target that does not exist in source
    #[arg(short = 'd', long)]
    delete_if_dne: bool,

    /// ignore dirs starting with specified string
    #[arg(short, long)]
    ignore_dir: Vec<Box<str>>,
}

#[derive(Debug, Subcommand)]
enum SubCmds {
    Pull(PullPushArgs),
    Push(PullPushArgs),
}

#[derive(Parser, Debug)]
#[command(
    help_template = "{author-with-newline}{about-section}Version: {version}\n{usage-heading} \
    {usage}\n{all-args} {tab}"
)]
#[command(arg_required_else_help(true))]
#[clap(version = "1.0", author = "github.com/j-hc")]
struct Cli {
    #[clap(subcommand)]
    subcmd: SubCmds,
}

type DirFileMap = HashMap<UnixPathBuf, HashSet<SyncFile>>;
fn get_dir_file_map(fs: Vec<SyncFile>, dir: &UnixPath) -> anyhow::Result<DirFileMap> {
    let mut dir_file_map: DirFileMap = HashMap::new();
    for f in fs {
        let mut p = f
            .path
            .strip_prefix(dir)
            .expect("has the prefix")
            .to_path_buf();
        p.pop();
        dir_file_map.entry(p).or_default().insert(f);
    }
    Ok(dir_file_map)
}

fn pull_push<SRC: FileSystem, DEST: FileSystem>(
    src_fs: &mut SRC,
    dest_fs: &mut DEST,
    PullPushArgs {
        source,
        dest,
        delete_if_dne,
        ignore_dir,
        set_times,
    }: PullPushArgs,
    adb_command: &'static str,
) -> anyhow::Result<()> {
    let source = UnixPathBuf::try_from(source).expect("source path");
    let dest = UnixPathBuf::try_from(dest).expect("dest path");

    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{} -> {}\n", source.display(), dest.display())?;

    let (src_files, src_empty_dirs) = src_fs.get_all_files(&source)?;
    let dir_file_map_android = get_dir_file_map(src_files, &source)?;

    let (dest_files, dest_empty_dirs) = dest_fs.get_all_files(&dest)?;
    let mut dir_file_map_local = get_dir_file_map(dest_files, &dest)?;

    let empty_hs = HashSet::new();
    for (path, androidfs) in dir_file_map_android {
        let localfs = dir_file_map_local.remove(&path);
        if ignore_dir.iter().any(|g| path.as_str().starts_with(&**g)) {
            writeln!(stdout, "SKIP DIR (IGNORED): {}", path.display()).unwrap();
            continue;
        }
        if localfs.is_none() {
            dest_fs.mkdir(&dest.join(&path))?;
        }

        for af in &androidfs {
            let lf = localfs.as_ref().and_then(|localfs| localfs.get(af));
            match lf {
                Some(lf) if af.size != lf.size => {
                    let op = adb_cmd!(adb_command, af.path, lf.path)?;
                    write!(stdout, "{adb_command} (SIZE) {op}")?;
                }
                Some(lf) if af.timestamp > lf.timestamp => {
                    let op = adb_cmd!(adb_command, af.path, lf.path)?;
                    write!(stdout, "{adb_command} (NEWER) {op}")?;
                }
                Some(_) => writeln!(stdout, "SKIP (OLDER): {}", af.path.display())?,
                None => {
                    let op = adb_cmd!(adb_command, af.path, dest.join(&path).join(&*af.name))?;
                    write!(stdout, "{adb_command} (DNE) {op}")?;
                }
            }
            if set_times {
                if let Some(lf) = lf {
                    dest_fs.set_mtime(&lf.path, af.timestamp)?;
                }
            }
        }
        if delete_if_dne {
            for sf_del in localfs.as_ref().unwrap_or(&empty_hs).difference(&androidfs) {
                writeln!(stdout, "DEL FILE: '{}'", sf_del.path.display())?;
                dest_fs.rm_file(&sf_del.path)?;
            }
        }
        writeln!(stdout)?;
    }
    for sf_dir_empty in &src_empty_dirs {
        let p = dest.join(sf_dir_empty.path.strip_prefix(&source)?.as_str());
        dest_fs.mkdir(&p)?;
        if set_times {
            dest_fs.set_mtime(&sf_dir_empty.path, sf_dir_empty.timestamp)?;
        }
    }
    if delete_if_dne {
        for remaining_local in dir_file_map_local.keys() {
            let p = dest.join(remaining_local);
            writeln!(stdout, "DEL DIR: '{}'", p.display())?;
            let _ = dest_fs
                .rm_dir(&p)
                .map_err(|e| println!("could not delete: '{}'", e));
        }

        let dest_empty_dirs_hs: HashSet<Box<UnixPath>> = HashSet::from_iter(
            dest_empty_dirs
                .into_iter()
                .map(|dp| dp.path.strip_prefix(&dest).unwrap().into()),
        );
        let src_empty_dirs_hs: HashSet<Box<UnixPath>> = HashSet::from_iter(
            src_empty_dirs
                .into_iter()
                .map(|sp| sp.path.strip_prefix(&source).unwrap().into()),
        );

        for sf_dest_dir_empty in dest_empty_dirs_hs.difference(&src_empty_dirs_hs) {
            let sf_dest_dir_empty = dest.join(sf_dest_dir_empty);
            writeln!(stdout, "DEL EMPTY DIR: '{}'", sf_dest_dir_empty.display())?;
            let _ = dest_fs
                .rm_dir(&sf_dest_dir_empty)
                .map_err(|e| println!("could not delete: '{}'", e));
        }
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    match adb_cmd!("devices") {
        Ok(devices) => {
            println!("{}\n", devices.trim());
            if devices
                .lines()
                .filter(|line| line.contains("\tdevice"))
                .count()
                > 1
            {
                anyhow::bail!("more than 1 device connected");
            }
        }
        Err(AdbErr::IO(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("adb binary not found")
        }
        Err(e) => anyhow::bail!("{}", e),
    }

    let mut android_fs = AndroidFS {
        shell: AdbShell::new()?,
    };
    match args.subcmd {
        SubCmds::Pull(p) => {
            if !p.dest.exists() {
                LocalFS.mkdir(&UnixPathBuf::try_from(p.dest.clone()).unwrap())?;
            }
            pull_push::<AndroidFS, LocalFS>(&mut android_fs, &mut LocalFS, p, "pull")?;
        }
        SubCmds::Push(p) => {
            pull_push::<LocalFS, AndroidFS>(&mut LocalFS, &mut android_fs, p, "push")?
        }
    }
    Ok(())
}
