use crate::adb::AdbCmd;
use crate::adb::AdbShell;
use crate::logw;
use crate::CResult;
use chainerror::Context;
use std::{
    fmt::Debug,
    fs::File,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use typed_path::Utf8UnixPath as UnixPath;

#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
#[cfg(target_os = "windows")]
use std::os::windows::fs::MetadataExt;

pub trait FileSystem {
    fn mkdir(&mut self, path: &UnixPath) -> CResult<()>;
    fn list_dir(&mut self, path: &UnixPath) -> CResult<Vec<SyncFile>>;
    fn rm(&mut self, path: &UnixPath) -> CResult<()>;
    fn rm_dir(&mut self, path: &UnixPath) -> CResult<()>;
    fn set_mtime(&mut self, path: &UnixPath, timestamp: u32) -> CResult<()>;
    fn get_all_files(&mut self, path: &UnixPath) -> CResult<(Vec<SyncFile>, Vec<SyncFile>)> {
        let mut fs = self.list_dir(path).annotate()?;
        let mut ffs = Vec::with_capacity(fs.len());
        let mut dirs = Vec::new();
        while let Some(f) = fs.pop() {
            match f.mode {
                FileMode::File => ffs.push(f),
                FileMode::Dir => {
                    let mut l = self.list_dir(&f.path).annotate()?;
                    if !l.is_empty() {
                        fs.append(&mut l);
                    } else {
                        dirs.push(f);
                    }
                }
                FileMode::Symlink => unreachable!("symlink: '{:?}'", f),
            }
        }
        Ok((ffs, dirs))
    }
}

pub struct AndroidFS {
    pub shell: AdbShell,
}

#[derive(Debug, Eq, Hash, PartialEq, Clone, Copy)]
pub enum FileMode {
    File,
    Dir,
    Symlink,
}

impl FileMode {
    pub fn from_u32(mode: u32) -> Self {
        match mode >> 13 {
            0b100 => Self::File,
            0b010 => Self::Dir,
            0b101 => Self::Symlink,
            _ => unreachable!("file mode? {}", mode),
        }
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct SyncFile {
    pub mode: FileMode,
    pub size: u32,
    pub timestamp: u32,
    pub name: Box<str>,
    pub path: Box<UnixPath>,
}

fn hex2u32(s: &str) -> u32 {
    match u32::from_str_radix(s, 16) {
        Ok(u) => u,
        Err(e) => panic!("{e:?} ({s})"),
    }
}

pub trait FSCopyFrom<SRC: FileSystem>: FileSystem {
    fn copy(&mut self, from: &UnixPath, to: &UnixPath, timestamp: Option<u32>) -> CResult<()>;

    // default is generic but slow compared to adb, doing with adb pull/push is better
    fn copy_dir(&mut self, from: &UnixPath, to: &UnixPath) -> CResult<()> {
        self.mkdir(to).annotate()?;
        for entry in self.list_dir(from).annotate()? {
            let to_path = to.join(&*entry.name);
            match entry.mode {
                FileMode::File => self.copy(&entry.path, &to_path, None).annotate()?,
                FileMode::Dir => self.copy_dir(&entry.path, &to_path).annotate()?,
                FileMode::Symlink => todo!(),
            }
        }
        Ok(())
    }
}

impl FSCopyFrom<LocalFS> for AndroidFS {
    fn copy(&mut self, from: &UnixPath, to: &UnixPath, timestamp: Option<u32>) -> CResult<()> {
        let mut cmd = AdbCmd::new();
        cmd.args(["push", from.as_str(), to.as_str()]);
        let _op = cmd.output().annotate()?;
        if let Some(timestamp) = timestamp {
            self.set_mtime(to, timestamp).annotate()?;
        }
        Ok(())
    }

    fn copy_dir(&mut self, from: &UnixPath, to: &UnixPath) -> CResult<()> {
        <AndroidFS as FSCopyFrom<LocalFS>>::copy(self, from, to, None).annotate()?;
        Ok(())
    }
}

impl FSCopyFrom<AndroidFS> for LocalFS {
    fn copy(&mut self, from: &UnixPath, to: &UnixPath, timestamp: Option<u32>) -> CResult<()> {
        let mut cmd = AdbCmd::new();
        cmd.args(["pull"]);
        if timestamp.is_some() {
            cmd.arg("-a");
        }
        cmd.args([from.as_str(), to.as_str()]);
        let _op = cmd.output().annotate()?;
        Ok(())
    }

    fn copy_dir(&mut self, from: &UnixPath, to: &UnixPath) -> CResult<()> {
        <LocalFS as FSCopyFrom<AndroidFS>>::copy(self, from, to, None).annotate()?;
        Ok(())
    }
}

impl FSCopyFrom<LocalFS> for LocalFS {
    fn copy(&mut self, from: &UnixPath, to: &UnixPath, _timestamp: Option<u32>) -> CResult<()> {
        std::fs::copy(from.as_str(), to.as_str()).annotate()?;
        Ok(())
    }
}

impl FileSystem for AndroidFS {
    fn mkdir(&mut self, _path: &UnixPath) -> CResult<()> {
        // adb push already does this
        // self.shell.run(["mkdir", "-p", path.as_str()]).annotate()?;
        Ok(())
    }

    fn list_dir(&mut self, path: &UnixPath) -> CResult<Vec<SyncFile>> {
        let op = AdbCmd::run_v(["ls", path.as_str()]).annotate()?;
        let mut files = Vec::with_capacity(op.lines().count());
        for line in op.lines() {
            let (s, line) = line.split_once(' ').expect("ls output mode");
            let mode = hex2u32(s);

            let (s, line) = line.split_once(' ').expect("ls output size");
            let size = hex2u32(s);

            let (s, name) = line.split_once(' ').expect("ls output epoch");
            if name == "." || name == ".." {
                continue;
            }
            let timestamp = hex2u32(s);
            let path = path.join(name);
            files.push(SyncFile {
                mode: FileMode::from_u32(mode),
                size,
                timestamp,
                name: name.into(),
                path: path.into(),
            });
        }

        Ok(files)
    }

    fn rm(&mut self, _path: &UnixPath) -> CResult<()> {
        logw!("ignoring AndroidFS::rm");
        Ok(())
    }

    fn rm_dir(&mut self, _path: &UnixPath) -> CResult<()> {
        logw!("ignoring AndroidFS::rm_dir");
        Ok(())
    }

    fn set_mtime(&mut self, _path: &UnixPath, mut _timestamp: u32) -> CResult<()> {
        // adb push already does this?
        Ok(())
        // let timestamp = timestamp.to_string();
        // let mut ts = String::with_capacity(1 + timestamp.len());
        // ts.push('@');
        // ts.push_str(&timestamp);
        // adb_shell!(self.shell, "touch", "-m", "-d", ts, path)?;
    }
}

pub struct LocalFS;
impl FileSystem for LocalFS {
    fn mkdir(&mut self, path: &UnixPath) -> CResult<()> {
        Ok(std::fs::create_dir_all(path.as_str()).annotate()?)
    }

    fn list_dir(&mut self, path: &UnixPath) -> CResult<Vec<SyncFile>> {
        let mut fs = Vec::new();
        for dir in std::fs::read_dir(path.as_str()).annotate()? {
            let dir = dir.annotate()?;
            let md = dir.metadata().annotate()?;
            let mode = if md.is_dir() {
                FileMode::Dir
            } else if md.is_file() {
                FileMode::File
            } else if md.is_symlink() {
                FileMode::Symlink
            } else {
                unreachable!("file mode?");
            };
            let name = dir
                .file_name()
                .into_string()
                .expect("file name is valid unicode");
            let path = path.join(&name);
            #[cfg(target_os = "windows")]
            let size = md.file_size() as u32;
            #[cfg(target_os = "linux")]
            let size = md.size() as u32;

            fs.push(SyncFile {
                mode,
                size,
                timestamp: md
                    .modified()
                    .annotate()?
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .expect("get system time")
                    .as_secs() as u32,
                name: name.into_boxed_str(),
                path: path.into_boxed_path(),
            });
        }
        Ok(fs)
    }

    fn rm(&mut self, path: &UnixPath) -> CResult<()> {
        Ok(std::fs::remove_file(path.as_str()).annotate()?)
    }

    fn rm_dir(&mut self, path: &UnixPath) -> CResult<()> {
        Ok(std::fs::remove_dir_all(path.as_str()).annotate()?)
    }

    fn set_mtime(&mut self, path: &UnixPath, timestamp: u32) -> CResult<()> {
        let dest = File::options().write(true).open(path.as_str()).annotate()?;
        dest.set_modified(UNIX_EPOCH + Duration::from_secs(timestamp as u64))
            .annotate()?;
        Ok(())
    }
}
