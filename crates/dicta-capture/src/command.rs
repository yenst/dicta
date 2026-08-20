use std::{
    env,
    ffi::{OsStr, OsString},
    io,
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::Duration,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandPlan {
    program: OsString,
    arguments: Vec<OsString>,
}

impl CommandPlan {
    #[must_use]
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            arguments: Vec::new(),
        }
    }

    #[must_use]
    pub fn arg(mut self, argument: impl Into<OsString>) -> Self {
        self.arguments.push(argument.into());
        self
    }

    pub fn push_arg(&mut self, argument: impl Into<OsString>) {
        self.arguments.push(argument.into());
    }

    #[must_use]
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    #[must_use]
    pub fn arguments(&self) -> &[OsString] {
        &self.arguments
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandOutput {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl CommandOutput {
    #[must_use]
    pub fn success(stdout: impl Into<Vec<u8>>) -> Self {
        Self {
            success: true,
            code: Some(0),
            stdout: stdout.into(),
            stderr: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessExit {
    pub success: bool,
    pub code: Option<i32>,
}

impl From<ExitStatus> for ProcessExit {
    fn from(status: ExitStatus) -> Self {
        Self {
            success: status.success(),
            code: status.code(),
        }
    }
}

pub trait CaptureChild: Send {
    fn id(&self) -> u32;
    /// Checks whether the child has exited without blocking.
    ///
    /// # Errors
    ///
    /// Returns an operating-system error when process state cannot be read.
    fn try_wait(&mut self) -> io::Result<Option<ProcessExit>>;
    /// Forcefully terminates the child.
    ///
    /// # Errors
    ///
    /// Returns an operating-system error when termination fails.
    fn kill(&mut self) -> io::Result<()>;
    /// Waits for the child to exit and reaps it.
    ///
    /// # Errors
    ///
    /// Returns an operating-system error when waiting fails.
    fn wait(&mut self) -> io::Result<ProcessExit>;
}

pub trait Platform {
    fn executable_exists(&self, name: &OsStr) -> bool;
    /// Runs a finite command and captures its output.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the command cannot run or be waited on.
    fn output(&mut self, plan: &CommandPlan) -> io::Result<CommandOutput>;
    /// Spawns a long-running process represented by the plan.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the process cannot be started.
    fn spawn(&mut self, plan: &CommandPlan) -> io::Result<Box<dyn CaptureChild>>;
    fn sleep(&mut self, duration: Duration);
}

#[derive(Debug, Default)]
pub struct SystemPlatform;

impl Platform for SystemPlatform {
    fn executable_exists(&self, name: &OsStr) -> bool {
        env::var_os("PATH").is_some_and(|path| {
            env::split_paths(&path).any(|directory| {
                let candidate: PathBuf = directory.join(name);
                candidate.is_file()
            })
        })
    }

    fn output(&mut self, plan: &CommandPlan) -> io::Result<CommandOutput> {
        let output = Command::new(plan.program())
            .args(plan.arguments())
            .stdin(Stdio::null())
            .output()?;
        Ok(CommandOutput {
            success: output.status.success(),
            code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    fn spawn(&mut self, plan: &CommandPlan) -> io::Result<Box<dyn CaptureChild>> {
        let child = Command::new(plan.program())
            .args(plan.arguments())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(Box::new(SystemChild(child)))
    }

    fn sleep(&mut self, duration: Duration) {
        thread::sleep(duration);
    }
}

struct SystemChild(Child);

impl CaptureChild for SystemChild {
    fn id(&self) -> u32 {
        self.0.id()
    }

    fn try_wait(&mut self) -> io::Result<Option<ProcessExit>> {
        self.0.try_wait().map(|status| status.map(Into::into))
    }

    fn kill(&mut self) -> io::Result<()> {
        self.0.kill()
    }

    fn wait(&mut self) -> io::Result<ProcessExit> {
        self.0.wait().map(Into::into)
    }
}
