use std::{
    io,
    process::{Child, Command, Stdio},
    thread,
};

use std::io::Write;

pub trait CommandExt<SO: Write + Send, SE: Write + Send> {
    fn spawn_and_stream(&mut self, stdoutw: SO, stderrw: SE) -> io::Result<Child>;
}

struct TeeWrite<A, B> {
    a: A,
    b: B,
}
impl<A: Write + Send, B: Write + Send> Write for TeeWrite<A, B> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.a.write_all(buf)?;
        self.b.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.a.flush()?;
        self.b.flush()
    }
}

impl<SO: Write + Send, SE: Write + Send> CommandExt<SO, SE> for Command {
    fn spawn_and_stream(&mut self, stdoutw: SO, mut stderrw: SE) -> io::Result<Child> {
        self.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .and_then(|mut c| {
                thread::scope(|scope| {
                    std::mem::take(&mut c.stdout)
                        .map(|mut stdout| {
                            scope.spawn(move || {
                                io::copy(
                                    &mut stdout,
                                    &mut TeeWrite {
                                        a: stdoutw,
                                        b: io::stdout(),
                                    },
                                )
                            })
                        })
                        .unwrap();
                    std::mem::take(&mut c.stderr)
                        .map(|mut stderr| scope.spawn(move || io::copy(&mut stderr, &mut stderrw)))
                        .unwrap();
                    Ok(c)
                })
            })
    }
}
