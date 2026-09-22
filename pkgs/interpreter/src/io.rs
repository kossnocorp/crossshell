use anyhow::{Result, bail};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    process::Command,
    sync::Arc,
};

/// Cloned descriptors share the underlying open-file description, just as dup
/// does. Replacing an entry never changes the embedding process's descriptors.
#[derive(Clone)]
pub(crate) struct Io(pub BTreeMap<u32, Arc<File>>);

impl Io {
    pub fn inherited() -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::fd::AsFd;
            Ok(Self(BTreeMap::from([
                (
                    0,
                    Arc::new(File::from(std::io::stdin().as_fd().try_clone_to_owned()?)),
                ),
                (
                    1,
                    Arc::new(File::from(std::io::stdout().as_fd().try_clone_to_owned()?)),
                ),
                (
                    2,
                    Arc::new(File::from(std::io::stderr().as_fd().try_clone_to_owned()?)),
                ),
            ])))
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsHandle;
            Ok(Self(BTreeMap::from([
                (
                    0,
                    Arc::new(File::from(
                        std::io::stdin().as_handle().try_clone_to_owned()?,
                    )),
                ),
                (
                    1,
                    Arc::new(File::from(
                        std::io::stdout().as_handle().try_clone_to_owned()?,
                    )),
                ),
                (
                    2,
                    Arc::new(File::from(
                        std::io::stderr().as_handle().try_clone_to_owned()?,
                    )),
                ),
            ])))
        }
    }

    pub fn get(&self, fd: u32) -> Result<Arc<File>> {
        self.0
            .get(&fd)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("{fd}: bad file descriptor"))
    }

    pub fn write(&self, fd: u32, text: impl AsRef<[u8]>) -> Result<()> {
        (&*self.get(fd)?).write_all(text.as_ref())?;
        Ok(())
    }

    pub fn read_line(&self) -> Result<Option<String>> {
        let file = self.get(0)?;
        let mut input = &*file;
        let mut line = Vec::new();
        let mut byte = [0];
        while input.read(&mut byte)? != 0 {
            if byte[0] == b'\n' {
                return Ok(Some(String::from_utf8(line)?));
            }
            line.push(byte[0]);
        }
        if line.is_empty() {
            Ok(None)
        } else {
            Ok(Some(String::from_utf8(line)?))
        }
    }

    pub fn configure(&self, command: &mut Command) -> Result<()> {
        command
            .stdin(self.get_stdio(0)?)
            .stdout(self.get_stdio(1)?)
            .stderr(self.get_stdio(2)?);
        #[cfg(unix)]
        {
            use command_fds::{CommandFdExt, FdMapping};
            use std::os::unix::process::CommandExt;
            let mappings = self
                .0
                .iter()
                .filter(|(fd, _)| **fd > 2)
                .map(|(fd, file)| {
                    Ok(FdMapping {
                        parent_fd: file.try_clone()?.into(),
                        child_fd: i32::try_from(*fd)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            command.fd_mappings(mappings)?;
            let closed: Vec<_> = (0..3).filter(|fd| !self.0.contains_key(fd)).collect();
            // SAFETY: only async-signal-safe close calls are made between fork
            // and exec. All allocation and descriptor selection happens above.
            unsafe {
                command.pre_exec(move || {
                    for fd in &closed {
                        libc::close(*fd as i32);
                    }
                    Ok(())
                });
            }
        }
        #[cfg(windows)]
        if self.0.keys().any(|fd| *fd > 2) {
            bail!("Extra file descriptors are not supported on Windows");
        }
        Ok(())
    }

    fn get_stdio(&self, fd: u32) -> Result<std::process::Stdio> {
        Ok(match self.0.get(&fd) {
            Some(f) => f.try_clone()?.into(),
            None => std::process::Stdio::null(),
        })
    }
}

pub(crate) fn pipe() -> Result<(Arc<File>, Arc<File>)> {
    let (reader, writer) = os_pipe::pipe()?;
    #[cfg(unix)]
    {
        use std::os::fd::OwnedFd;
        Ok((
            Arc::new(File::from(OwnedFd::from(reader))),
            Arc::new(File::from(OwnedFd::from(writer))),
        ))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::OwnedHandle;
        Ok((
            Arc::new(File::from(OwnedHandle::from(reader))),
            Arc::new(File::from(OwnedHandle::from(writer))),
        ))
    }
}

pub(crate) fn input(text: &str) -> Result<Arc<File>> {
    // A seekable anonymous file avoids blocking on a full pipe before a command
    // has been spawned, including arbitrarily large here-documents.
    let mut file = tempfile::tempfile()?;
    file.write_all(text.as_bytes())?;
    file.seek(SeekFrom::Start(0))?;
    Ok(Arc::new(file))
}

pub(crate) fn status(status: std::process::ExitStatus) -> u8 {
    if let Some(code) = status.code() {
        return code as u8;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal as u8;
        }
    }
    1
}

pub(crate) fn join<T>(handle: std::thread::ScopedJoinHandle<'_, Result<T>>) -> Result<T> {
    match handle.join() {
        Ok(result) => result,
        Err(_) => bail!("Interpreter worker panicked"),
    }
}
