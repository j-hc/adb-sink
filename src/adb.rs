use std::error::Error;
use std::fmt::{Debug, Display};
use std::io::BufReader;
use std::io::BufWriter;
use std::process::{ChildStdin, ChildStdout};
use std::{io, process::Command};

#[derive(Debug)]
pub enum AdbErr {
    IO(io::Error),
    Adb(Box<str>),
}

impl Display for AdbErr {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        Debug::fmt(self, f)
    }
}
impl Error for AdbErr {}
impl From<io::Error> for AdbErr {
    fn from(e: io::Error) -> Self {
        Self::IO(e)
    }
}
impl From<Box<str>> for AdbErr {
    fn from(e: Box<str>) -> Self {
        Self::Adb(e)
    }
}
impl From<String> for AdbErr {
    fn from(e: String) -> Self {
        Self::Adb(e.into_boxed_str())
    }
}

#[macro_export]
macro_rules! adb_cmd {
    ($($arg:expr),+ $(,)?) => {{
        print!("[ADB] ");
        $(print!("'{}' ", &$arg);)+
        println!();
        $crate::adb_cmd_q!($($arg),+)
    }}
}

#[macro_export]
macro_rules! adb_cmd_q {
    ($($arg:expr),+ $(,)?) => {(|| -> Result<String, AdbErr>{
        let mut op = ::std::process::Command::new("adb");
        op.stdout(::std::process::Stdio::piped()).stderr(::std::process::Stdio::piped());
        $(op.arg(&$arg);)+
        let op = op.output()?;
        if !op.stderr.is_empty() {
            Err(AdbErr::from(String::from_utf8(op.stderr).expect("utf8 output")))
        } else {
            let op = String::from_utf8(op.stdout).expect("utf8 output");
            if op.starts_with("adb: error:") {
                Err(AdbErr::from(op))
            } else {
                Ok(op)
            }
        }
    })()}
}

#[macro_export]
macro_rules! adb_shell {
    ($shell:expr,$($arg:expr),+ $(,)?) => {(|| -> Result<String, AdbErr> {
        print!("[ABD SHELL] ");
        $(print!("'{}' ",&$arg);)+
        println!();
        macro_rules! CMD_END {() => {"ADBSYNCEND"};}
        $(write!($shell.si, "{} ", $arg)?;)+
        $shell.si
            .write_all(concat!(";echo ", CMD_END!(), "\n").as_bytes())?;
        $shell.si.flush()?;
        let mut buf = ::std::string::String::new();
        while {
            $shell.so.read_line(&mut buf)?;
            let buf = buf.trim_end();
            if CMD_END!().len() > buf.len() {
                true
            } else {
                !buf.get(buf.len() - CMD_END!().len()..)
                    .is_some_and(|b| b == CMD_END!())
            }
        } {}
        buf.truncate(buf.len() - CMD_END!().len());
        Ok(buf)
    })()}
}

pub struct AdbShell {
    pub si: BufWriter<ChildStdin>,
    pub so: BufReader<ChildStdout>,
}

impl AdbShell {
    pub fn new() -> anyhow::Result<Self> {
        let mut c = Command::new("adb")
            .arg("shell")
            .stdin(::std::process::Stdio::piped())
            .stdout(::std::process::Stdio::piped())
            .spawn()?;
        Ok(Self {
            si: BufWriter::new(c.stdin.take().expect("si piped")),
            so: BufReader::new(c.stdout.take().expect("so piped")),
        })
    }
}

// pub struct AdbCmd {
//     cmd: Command,
// }

// impl AdbCmd {
//     pub fn run<I, S>(args: I) -> Result<String, AdbErr>
//     where
//         I: IntoIterator<Item = S>,
//         S: AsRef<OsStr>,
//     {
//         let mut cmd = Command::new("adb");
//         cmd.stdout(::std::process::Stdio::piped())
//             .stderr(::std::process::Stdio::piped());
//         cmd.args(args);
//         Self { cmd }.output()
//     }

//     pub fn new() -> Self {
//         let mut cmd = Command::new("adb");
//         cmd.stdout(::std::process::Stdio::piped())
//             .stderr(::std::process::Stdio::piped());
//         Self { cmd }
//     }

//     pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Self {
//         self.cmd.arg(arg.as_ref());
//         self
//     }

//     pub fn args<I, S>(&mut self, args: I) -> &mut Self
//     where
//         I: IntoIterator<Item = S>,
//         S: AsRef<OsStr>,
//     {
//         self.cmd.args(args);
//         self
//     }

//     pub fn output(&mut self) -> Result<String, AdbErr> {
//         let op = self.cmd.output()?;
//         if !op.stderr.is_empty() {
//             Err(AdbErr::from(
//                 String::from_utf8(op.stderr).expect("utf8 output"),
//             ))
//         } else {
//             let op = String::from_utf8(op.stdout).expect("utf8 output");
//             if op.contains("error") {
//                 Err(AdbErr::from(op))
//             } else {
//                 Ok(op)
//             }
//         }
//     }
// }
