use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommandOutput {
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitWorkerError {
    EmptyAddPaths,
    SpawnFailed { command: String, message: String },
    CommandFailed { command: String, code: Option<i32>, stderr: String },
}

impl Display for GitWorkerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            GitWorkerError::EmptyAddPaths => write!(f, "git add requires at least one path"),
            GitWorkerError::SpawnFailed { command, message } => {
                write!(f, "failed to run `{command}`: {message}")
            }
            GitWorkerError::CommandFailed { command, code, stderr } => {
                write!(f, "`{command}` failed with code {:?}: {}", code, stderr.trim())
            }
        }
    }
}

impl Error for GitWorkerError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub trait CommandExecutor: Send + Sync {
    fn execute(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<CommandResult, std::io::Error>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessCommandExecutor;

impl CommandExecutor for ProcessCommandExecutor {
    fn execute(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<CommandResult, std::io::Error> {
        let output = Command::new(program).args(args).current_dir(cwd).output()?;
        Ok(CommandResult {
            success: output.status.success(),
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct GitWorker<E = ProcessCommandExecutor> {
    repo_path: PathBuf,
    executor: E,
}

impl GitWorker<ProcessCommandExecutor> {
    pub fn new(repo_path: impl Into<PathBuf>) -> Self {
        Self { repo_path: repo_path.into(), executor: ProcessCommandExecutor }
    }
}

impl<E: CommandExecutor> GitWorker<E> {
    pub fn with_executor(repo_path: impl Into<PathBuf>, executor: E) -> Self {
        Self { repo_path: repo_path.into(), executor }
    }

    pub fn status(&self) -> Result<GitCommandOutput, GitWorkerError> {
        self.run(vec!["status".to_string(), "--short".to_string()])
    }

    pub fn add<S: AsRef<str>>(&self, paths: &[S]) -> Result<GitCommandOutput, GitWorkerError> {
        if paths.is_empty() {
            return Err(GitWorkerError::EmptyAddPaths);
        }

        let mut args = vec!["add".to_string()];
        args.extend(paths.iter().map(|path| path.as_ref().to_string()));
        self.run(args)
    }

    pub fn commit(&self, message: &str) -> Result<GitCommandOutput, GitWorkerError> {
        self.run(vec!["commit".to_string(), "-m".to_string(), message.to_string()])
    }

    pub fn diff_cached(&self) -> Result<GitCommandOutput, GitWorkerError> {
        self.run(vec!["diff".to_string(), "--cached".to_string(), "--no-color".to_string()])
    }

    pub fn diff_cached_name_status(&self) -> Result<GitCommandOutput, GitWorkerError> {
        self.run(vec!["diff".to_string(), "--cached".to_string(), "--name-status".to_string()])
    }

    pub fn push(&self) -> Result<GitCommandOutput, GitWorkerError> {
        self.run(vec!["push".to_string()])
    }

    pub fn pull_rebase(&self) -> Result<GitCommandOutput, GitWorkerError> {
        self.run(vec!["pull".to_string(), "--rebase".to_string()])
    }

    fn run(&self, args: Vec<String>) -> Result<GitCommandOutput, GitWorkerError> {
        let command = format!("git {}", args.join(" "));
        let result = self.executor.execute("git", &args, &self.repo_path).map_err(|error| {
            GitWorkerError::SpawnFailed { command: command.clone(), message: error.to_string() }
        })?;

        if result.success {
            return Ok(GitCommandOutput { stdout: result.stdout, stderr: result.stderr });
        }

        let stderr = if result.stderr.trim().is_empty() { result.stdout } else { result.stderr };

        Err(GitWorkerError::CommandFailed { command, code: result.code, stderr })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Invocation {
        program: String,
        args: Vec<String>,
        cwd: PathBuf,
    }

    #[derive(Clone)]
    struct MockExecutor {
        calls: Arc<Mutex<Vec<Invocation>>>,
        responses: Arc<Mutex<VecDeque<Result<CommandResult, std::io::Error>>>>,
    }

    impl MockExecutor {
        fn new(responses: Vec<Result<CommandResult, std::io::Error>>) -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                responses: Arc::new(Mutex::new(VecDeque::from(responses))),
            }
        }

        fn calls(&self) -> Vec<Invocation> {
            self.calls.lock().expect("mock calls lock poisoned").clone()
        }
    }

    impl CommandExecutor for MockExecutor {
        fn execute(
            &self,
            program: &str,
            args: &[String],
            cwd: &Path,
        ) -> Result<CommandResult, std::io::Error> {
            self.calls.lock().expect("mock calls lock poisoned").push(Invocation {
                program: program.to_string(),
                args: args.to_vec(),
                cwd: cwd.to_path_buf(),
            });

            self.responses
                .lock()
                .expect("mock responses lock poisoned")
                .pop_front()
                .expect("missing mock response")
        }
    }

    #[test]
    fn status_runs_git_status_short() {
        let mock = MockExecutor::new(vec![Ok(CommandResult {
            success: true,
            code: Some(0),
            stdout: " M README.md\n".to_string(),
            stderr: String::new(),
        })]);

        let worker = GitWorker::with_executor("/tmp/repo", mock.clone());
        let output = worker.status().expect("status should succeed");

        assert_eq!(output.stdout, " M README.md\n");
        let calls = mock.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "git");
        assert_eq!(calls[0].args, vec!["status", "--short"]);
        assert_eq!(calls[0].cwd, PathBuf::from("/tmp/repo"));
    }

    #[test]
    fn add_requires_at_least_one_path() {
        let mock = MockExecutor::new(Vec::new());
        let worker = GitWorker::with_executor("/tmp/repo", mock);

        let error = worker.add::<&str>(&[]).expect_err("add should fail");
        assert_eq!(error, GitWorkerError::EmptyAddPaths);
    }

    #[test]
    fn pull_rebase_returns_command_failure_with_stderr() {
        let mock = MockExecutor::new(vec![Ok(CommandResult {
            success: false,
            code: Some(1),
            stdout: String::new(),
            stderr: "fatal: bad revision\n".to_string(),
        })]);
        let worker = GitWorker::with_executor("/tmp/repo", mock.clone());

        let error = worker.pull_rebase().expect_err("pull --rebase should fail");
        assert_eq!(
            error,
            GitWorkerError::CommandFailed {
                command: "git pull --rebase".to_string(),
                code: Some(1),
                stderr: "fatal: bad revision\n".to_string(),
            }
        );

        let calls = mock.calls();
        assert_eq!(calls[0].args, vec!["pull", "--rebase"]);
    }

    #[test]
    fn commit_passes_message_as_single_argument() {
        let mock = MockExecutor::new(vec![Ok(CommandResult {
            success: true,
            code: Some(0),
            stdout: "[main abc123] update\n".to_string(),
            stderr: String::new(),
        })]);
        let worker = GitWorker::with_executor("/tmp/repo", mock.clone());

        let _ = worker.commit("docs: update readme section").expect("commit should succeed");

        let calls = mock.calls();
        assert_eq!(calls[0].args, vec!["commit", "-m", "docs: update readme section"]);
    }

    #[test]
    fn diff_commands_use_cached_flags() {
        let mock = MockExecutor::new(vec![
            Ok(CommandResult {
                success: true,
                code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            }),
            Ok(CommandResult {
                success: true,
                code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            }),
        ]);
        let worker = GitWorker::with_executor("/tmp/repo", mock.clone());

        let _ = worker.diff_cached().expect("diff --cached should succeed");
        let _ = worker.diff_cached_name_status().expect("diff --name-status should succeed");

        let calls = mock.calls();
        assert_eq!(calls[0].args, vec!["diff", "--cached", "--no-color"]);
        assert_eq!(calls[1].args, vec!["diff", "--cached", "--name-status"]);
    }
}
