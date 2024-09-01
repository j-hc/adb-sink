use crate::adb::AdbCmd;
use crate::adb::AdbShell;
use crate::Result;
use chainerror::Context;
use std::{
    fmt::Debug,
    fs::File,
    hash::Hash,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use typed_path::UnixPath;

#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
#[cfg(target_os = "windows")]
use std::os::windows::fs::MetadataExt;

pub trait AsStr {
    fn as_str(&self) -> &str;
}

impl AsStr for UnixPath {
    fn as_str(&self) -> &str {
        self.to_str().expect("path to str")
    }
}

pub trait FileSystem {
    fn mkdir(&mut self, path: &UnixPath) -> Result<()>;
    fn list_dir(&mut self, path: &UnixPath) -> Result<Vec<SyncFile>>;
    fn rm(&mut self, path: &UnixPath) -> Result<()>;
    fn rm_dir(&mut self, path: &UnixPath) -> Result<()>;
    fn set_mtime(&mut self, path: &UnixPath, timestamp: u32) -> Result<()>;
    fn get_all_files(&mut self, path: &UnixPath) -> Result<(Vec<SyncFile>, Vec<SyncFile>)> {
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
            _ => unreachable!("undefined file mode {}", mode),
        }
    }
}

#[derive(Eq, Clone)]
pub struct SyncFile {
    pub mode: FileMode,
    pub size: u32,
    pub timestamp: u32,
    pub name: Box<str>,
    pub path: Box<UnixPath>,
}

impl PartialEq for SyncFile {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl Hash for SyncFile {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl Debug for SyncFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncFile")
            .field("mode", &self.mode)
            .field("size", &self.size)
            .field("timestamp", &self.timestamp)
            .field("name", &self.name)
            .field("path", &self.path.as_str())
            .finish()
    }
}

fn hex2u32(s: &str) -> u32 {
    match u32::from_str_radix(s, 16) {
        Ok(u) => u,
        Err(e) => panic!("{e:?} ({s})"),
    }
}

pub trait FSCopyFrom<SRC: FileSystem> {
    fn copy(&mut self, from: &UnixPath, to: &UnixPath, timestamp: Option<u32>) -> Result<()>;
}

impl FSCopyFrom<LocalFS> for AndroidFS {
    fn copy(&mut self, from: &UnixPath, to: &UnixPath, timestamp: Option<u32>) -> Result<()> {
        let mut cmd = AdbCmd::new();
        cmd.args(["push", "-z", "any", from.as_str(), to.as_str()]);
        let _op = cmd.output().annotate()?;
        if let Some(timestamp) = timestamp {
            self.set_mtime(to, timestamp).annotate()?;
        }
        Ok(())
    }
}

impl FSCopyFrom<AndroidFS> for LocalFS {
    fn copy(&mut self, from: &UnixPath, to: &UnixPath, timestamp: Option<u32>) -> Result<()> {
        let mut cmd = AdbCmd::new();
        cmd.args(["pull", "-z", "any"]);
        if timestamp.is_some() {
            cmd.arg("-a");
        }
        cmd.args([from.as_str(), to.as_str()]);
        let _op = cmd.output().annotate()?;
        Ok(())
    }
}

impl FSCopyFrom<LocalFS> for LocalFS {
    fn copy(&mut self, from: &UnixPath, to: &UnixPath, _timestamp: Option<u32>) -> Result<()> {
        std::fs::copy(from.as_str(), to.as_str()).annotate()?;
        Ok(())
    }
}

impl FileSystem for AndroidFS {
    fn mkdir(&mut self, _path: &UnixPath) -> Result<()> {
        // adb push already does this
        // self.shell.run(["mkdir", "-p", path.as_str()]).annotate()?;
        Ok(())
    }

    fn list_dir(&mut self, path: &UnixPath) -> Result<Vec<SyncFile>> {
        let op = AdbCmd::run_v(["ls", path.as_str()]).annotate()?;
        let mut files = Vec::with_capacity(op.lines().count());
        for line in op.lines() {
            let (s, line) = line.split_once(' ').expect("ls output no mode");
            let mode = hex2u32(s);

            let (s, line) = line.split_once(' ').expect("ls output no size");
            let size = hex2u32(s);

            let (s, name) = line.split_once(' ').expect("ls output no epoch");
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

    fn rm(&mut self, _path: &UnixPath) -> Result<()> {
        unimplemented!("dont delete in device for now");
        // adb_shell!(self.shell, "rm", path)?;
        // Ok(())
    }

    fn rm_dir(&mut self, _path: &UnixPath) -> Result<()> {
        unimplemented!("dont delete in device for now");
        // adb_shell!(self.shell, "rm", "-r", path)?;
        // Ok(())
    }

    fn set_mtime(&mut self, _path: &UnixPath, mut _timestamp: u32) -> Result<()> {
        // adb push already does this?
        Ok(())
        // let timestamp = timestamp.to_string();
        // let mut ts = String::with_capacity(1 + timestamp.len);
        // ts.push('@');
        // ts.push_str(&timestamp);
        // adb_shell!(self.shell, "touch", "-m", "-d", ts, path)?;
    }
}

pub struct LocalFS;
impl FileSystem for LocalFS {
    fn mkdir(&mut self, path: &UnixPath) -> Result<()> {
        Ok(std::fs::create_dir_all(path.as_str()).annotate()?)
    }

    fn list_dir(&mut self, path: &UnixPath) -> Result<Vec<SyncFile>> {
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
            let name = dir.file_name().into_string().unwrap();
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
                    .expect("system time shouldnt error")
                    .as_secs() as u32,
                name: name.into_boxed_str(),
                path: path.into(),
            });
        }
        Ok(fs)
    }

    fn rm(&mut self, path: &UnixPath) -> Result<()> {
        Ok(std::fs::remove_file(path.as_str()).annotate()?)
    }

    fn rm_dir(&mut self, path: &UnixPath) -> Result<()> {
        Ok(std::fs::remove_dir_all(path.as_str()).annotate()?)
    }

    fn set_mtime(&mut self, path: &UnixPath, timestamp: u32) -> Result<()> {
        let dest = File::options().write(true).open(path.as_str()).annotate()?;
        dest.set_modified(UNIX_EPOCH + Duration::from_secs(timestamp as u64))
            .annotate()?;
        Ok(())
    }
}
