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
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
    () => {};
    ($($arg:expr),+ $(,)?) => {(|| -> Result<String, AdbErr>{
        print!("[ABD] ");
        $(print!("{} ", &$arg.to_string());)+
        println!();
        let mut op = ::std::process::Command::new("adb");
        op.stdout(::std::process::Stdio::piped()).stderr(::std::process::Stdio::piped());
        $(op.arg(&$arg.to_string());)+
        let op = op.output()?;
        if !op.stderr.is_empty() {
            Err(AdbErr::from(String::from_utf8(op.stderr).expect("utf8 output")))
        } else {
            let op = String::from_utf8(op.stdout).expect("utf8 output");
            if op.contains("error") {
                Err(AdbErr::from(op))
            } else {
                Ok(op)
            }
        }
    })()}
}

#[macro_export]
macro_rules! adb_shell {
    () => {};
    ($shell:expr,$($arg:expr),+ $(,)?) => {(|| -> Result<String, AdbErr> {
        print!("[ABD SHELL] ");
        $(print!("{} ", &$arg.to_string());)+
        println!();
        macro_rules! CMD_END {() => {"ADBSYNCEND"};}
        const CMD_END_CRLF: &str = concat!(CMD_END!(), "\n");
        $(write!($shell.si, "{} ", $arg)?;)+
        $shell.si
            .write_all(concat!(";echo ", CMD_END!(), "\n").as_bytes())?;
        $shell.si.flush()?;
        let mut buf = ::std::string::String::new();
        while {
            $shell.so.read_line(&mut buf)?;
            if CMD_END_CRLF.len() > buf.len() {
                true
            } else {
                !buf.get(buf.len() - CMD_END_CRLF.len()..)
                    .is_some_and(|b| b == CMD_END_CRLF)
            }
        } {}
        buf.truncate(buf.len() - CMD_END_CRLF.len());
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
