pub mod adb;
pub mod args;
pub mod fs;
pub mod tree;

use adb::AdbCmd;
use chainerror::Context;
use fs::{FSCopyFrom, FileMode, FileSystem, SyncFile};
use std::path::PathBuf;
use std::sync::OnceLock;
use tree::{build_tree, diff_trees};
use typed_path::Utf8UnixPathBuf as UnixPathBuf;

pub static VERBOSE: OnceLock<bool> = OnceLock::new();

pub fn is_verbose() -> bool {
    *VERBOSE.get().expect("set in main")
}

pub type CResult<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

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

pub fn sink<SRC: FileSystem, DEST: FileSystem + FSCopyFrom<SRC>>(
    src_fs: &mut SRC,
    dest_fs: &mut DEST,
    src_path: PathBuf,
    dst_path: PathBuf,
    delete_if_dne: bool,
    ignore_dirs: Vec<Box<str>>,
    set_time: bool,
) -> CResult<()> {
    let source_file_name = src_path.file_name().unwrap().to_str().unwrap().to_string();
    let dest_file_name = dst_path.file_name().unwrap().to_str().unwrap().to_string();

    let src_path = UnixPathBuf::from(src_path.to_str().unwrap());
    let dst_path = UnixPathBuf::from(dst_path.to_str().unwrap()).join(&source_file_name);
    dest_fs.mkdir(&dst_path).annotate()?;

    let src_root = build_tree(
        src_fs,
        SyncFile {
            mode: FileMode::Dir,
            size: 0,
            timestamp: 0,
            name: source_file_name.into_boxed_str(),
            path: src_path.clone().into_boxed_path(),
        },
        &src_path,
    )
    .annotate()?;
    let dest_root = build_tree(
        dest_fs,
        SyncFile {
            mode: FileMode::Dir,
            size: 0,
            timestamp: 0,
            name: dest_file_name.into_boxed_str(),
            path: dst_path.clone().into_boxed_path(),
        },
        &dst_path,
    )
    .annotate()?;

    let (dest_doesnt_have, src_doesnt_have, both_have_files) = diff_trees(&dest_root, &src_root);

    if delete_if_dne {
        for n in &src_doesnt_have {
            match n.sf.mode {
                FileMode::File => {
                    logi!("DEL FILE: '{}'", n.sf.path);
                    dest_fs.rm(&n.sf.path)
                }
                FileMode::Dir => {
                    logi!("DEL DIR: '{}'", n.sf.path);
                    dest_fs.rm_dir(&n.sf.path)
                }
                FileMode::Symlink => todo!(),
            }
            .annotate()?
        }
    }

    for n in &dest_doesnt_have {
        let from = src_path.join(&n.strip_path);
        let to = dst_path.join(&n.strip_path);
        if ignore_dirs.iter().any(|g| n.strip_path.starts_with(&**g)) {
            logi!("SKIP DIR (IGNORED): {}", from);
            continue;
        }
        match n.sf.mode {
            FileMode::File => {
                logi!("COPY FILE (DNE): {} -> {}", to, from);
                if cfg!(target_os = "windows") && n.sf.name.ends_with('.') {
                    logw!(
                        "Windows does not support file names ending with a dot: {}",
                        n.sf.name
                    );
                }
                dest_fs.copy(&from, &to, None)
            }
            FileMode::Dir => {
                logi!("COPY DIR (DNE): {} -> {}", to, from);
                dest_fs.copy_dir(&from, &to)
            }
            FileMode::Symlink => todo!(),
        }
        .annotate()?;
    }

    for (dest_file, src_file) in &both_have_files {
        let reason = if dest_file.size != src_file.size {
            "SIZE"
        } else if src_file.timestamp > dest_file.timestamp {
            "NEWER"
        } else {
            logv!("SKIP: '{}'", src_file.path);
            continue;
        };
        logi!(
            "COPY FILE ({reason}): {} -> {}",
            src_file.path,
            dest_file.path
        );
        dest_fs
            .copy(
                &src_file.path,
                &dest_file.path,
                if set_time {
                    Some(src_file.timestamp)
                } else {
                    None
                },
            )
            .annotate()?;

        if cfg!(target_os = "windows") && dest_file.name.ends_with('.') {
            logw!(
                "Windows does not support file names ending with a dot: {}",
                src_file.name
            );
        }
    }
    Ok(())
}

pub fn adb_connect() -> CResult<bool> {
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
    let mdns = mdns_sd::ServiceDaemon::new().expect("create daemon");
    let receiver = mdns.browse("_adb-tls-connect._tcp.local.").expect("browse");
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
