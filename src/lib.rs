pub mod adb;
pub mod args;
pub mod fs;

use adb::AdbCmd;
use chainerror::Context;
use fs::{AsStr, FSCopyFrom, FileSystem, SyncFile};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::OnceLock;
use typed_path::{UnixPath, UnixPathBuf};

pub static VERBOSE: OnceLock<bool> = OnceLock::new();

pub fn is_verbose() -> bool {
    *VERBOSE.get().expect("set in main")
}

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[macro_export]
macro_rules! logi {
    ($($arg:tt)*) => {{
		print!("[INFO] ");
        println!($($arg)*);
    }};
}

#[macro_export]
macro_rules! logw {
    ($($arg:tt)*) => {{
		print!("[WARN] ");
        println!($($arg)*);
    }};
}

#[macro_export]
macro_rules! logv {
    ($($arg:tt)*) => {{
        if is_verbose() {
            print!("[VERBOSE] ");
            println!($($arg)*);
        }
    }};
}

type DirFileMap = HashMap<UnixPathBuf, HashSet<SyncFile>>;
fn get_dir_file_map(fs: Vec<SyncFile>, dir: &UnixPath) -> DirFileMap {
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
    dir_file_map
}

pub fn sink<SRC: FileSystem, DEST: FileSystem + FSCopyFrom<SRC>>(
    src_fs: &mut SRC,
    dest_fs: &mut DEST,

    source: PathBuf,
    dest: PathBuf,
    delete_if_dne: bool,
    ignore_dir: Vec<Box<str>>,
    set_times: bool,
) -> Result<()> {
    let source_file_name = source.file_name().unwrap().to_str().unwrap().to_string();

    let source = typed_path::PathBuf::<typed_path::NativeEncoding>::try_from(source)
        .unwrap()
        .with_unix_encoding();
    let mut dest = typed_path::PathBuf::<typed_path::NativeEncoding>::try_from(dest)
        .unwrap()
        .with_unix_encoding();

    dest.push(source_file_name);
    dest_fs.mkdir(&dest).annotate()?;

    logi!("{} -> {}\n", source.display(), dest.display());
    let (src_files, mut src_empty_dirs) = src_fs.get_all_files(&source).annotate()?;
    src_empty_dirs.retain(|dir| {
        !ignore_dir.iter().any(|g| {
            dir.path
                .strip_prefix(&source)
                .unwrap()
                .as_str()
                .starts_with(&**g)
        })
    });
    let dir_file_map_src = get_dir_file_map(src_files, &source);

    let (dest_files, dest_empty_dirs) = dest_fs.get_all_files(&dest).annotate()?;

    let empty_dirs_hs = |empty_dirs: &[SyncFile], prefix: &UnixPath| -> HashSet<Box<UnixPath>> {
        HashSet::from_iter(
            empty_dirs
                .iter()
                .map(|p| p.path.strip_prefix(prefix).unwrap().into()),
        )
    };
    let mut dest_empty_dirs_hs = empty_dirs_hs(&dest_empty_dirs, &dest);
    let mut dir_file_map_dest = get_dir_file_map(dest_files, &dest);

    for (path, src_files) in dir_file_map_src {
        dest_empty_dirs_hs.remove(&*path);

        let dest_files = dir_file_map_dest.remove(&path);
        if ignore_dir.iter().any(|g| path.as_str().starts_with(&**g)) {
            logi!("SKIP DIR (IGNORED): {}", path.display());
            continue;
        }
        if dest_files.is_none() {
            dest_fs.mkdir(&dest.join(&path)).annotate()?;
        }

        for af in &src_files {
            let (lf_path, reason) = match dest_files
                .as_ref()
                .and_then(|dest_files| dest_files.get(af))
            {
                Some(lf) if af.size != lf.size => (&lf.path, "SIZE"),
                Some(lf) if af.timestamp > lf.timestamp => (&lf.path, "NEWER"),
                Some(_) => {
                    logv!("SKIP: '{}'", af.path.display());
                    continue;
                }
                None => (&dest.join(&path).join(&*af.name).into(), "DNE"),
            };

            logi!("- COPY ({reason}): {} -> {}", af.path, lf_path);
            dest_fs
                .copy(
                    &af.path,
                    lf_path,
                    if set_times { Some(af.timestamp) } else { None },
                )
                .annotate()?;

            #[cfg(target_os = "windows")]
            if af.name.ends_with('.') {
                logw!(
                    "Windows does not support file names ending with a dot: {}",
                    af.name
                );
            }
        }
        if delete_if_dne {
            if let Some(dest_files) = dest_files {
                for sf_del in dest_files.difference(&src_files) {
                    // TODO: handle files ending with '.' in windows
                    logi!("DEL (DNE): '{}'", sf_del.path.display());
                    dest_fs.rm(&sf_del.path).annotate()?;
                }
            }
        }
    }

    let src_empty_dirs_hs = empty_dirs_hs(&src_empty_dirs, &source);
    for sf_dest_dir_empty in src_empty_dirs_hs.difference(&dest_empty_dirs_hs) {
        let p = dest.join(sf_dest_dir_empty);
        logi!("CRETE EMPTY DIR: '{}'", p.display());
        dest_fs.mkdir(&p).annotate()?;
    }

    if delete_if_dne {
        for remaining_local_dir in dir_file_map_dest.keys() {
            let p = dest.join(remaining_local_dir);
            logi!("CLEAR DIR: '{}'", p.display());
            dest_fs.rm_dir(&p).annotate()?;
            dest_fs.mkdir(&p).annotate()?;
        }

        for sf_dest_dir_empty in dest_empty_dirs_hs
            .difference(&src_empty_dirs_hs)
            .map(|p| dest.join(p))
        {
            #[cfg(debug_assertions)]
            if std::fs::read_dir(sf_dest_dir_empty.to_str().unwrap())
                .annotate()?
                .next()
                .is_some()
            {
                unreachable!();
            }

            logi!("DEL DIR (DNE): '{}'", sf_dest_dir_empty.display());
            dest_fs.rm_dir(&sf_dest_dir_empty).annotate()?;
        }
    }
    Ok(())
}

pub fn adb_connect() -> Result<bool> {
    let devices = AdbCmd::run_v(["devices"]).annotate()?;
    match devices
        .lines()
        .filter(|line| line.contains("\tdevice"))
        .inspect(|line| logv!("{}", line))
        .count()
    {
        0 => {
            #[cfg(feature = "mdns")]
            if let Some((ip, port)) = mdns_discover() {
                logi!("Discovered device {} {}. Trying to connect...", ip, port);
                if AdbCmd::run_v(["connect", &format!("{}:{}", ip, port)])
                    .annotate()?
                    .starts_with("connected to")
                {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        1 => Ok(true),
        n if n > 1 => panic!("more than 1 device connected"),
        _ => unreachable!(),
    }
}

// not using adb's mdns since its disabled in most linux distros
#[cfg(feature = "mdns")]
fn mdns_discover() -> Option<(std::net::Ipv4Addr, u16)> {
    let mdns = mdns_sd::ServiceDaemon::new().expect("Failed to create daemon");
    let receiver = mdns
        .browse("_adb-tls-connect._tcp.local.")
        .expect("Failed to browse");
    let now = std::time::Instant::now();
    while let Ok(event) = receiver.recv() {
        match event {
            mdns_sd::ServiceEvent::ServiceResolved(info) => {
                let port = info.get_port();
                let addrs = info.get_addresses_v4();
                assert!(addrs.len() == 1);
                return Some((**addrs.iter().next().unwrap(), port));
            }
            _ => {
                if now.elapsed() > std::time::Duration::from_secs(3) {
                    return None;
                }
            }
        }
    }
    None
}
