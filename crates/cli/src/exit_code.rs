// Consistent exit codes for the scriptum CLI.
//
//   0  = success
//   1  = general error
//   2  = usage/argument error
//   10 = daemon not reachable
//   11 = authentication error
//   12 = conflict/overlap detected
//   13 = network error

use std::process;

/// Named exit codes for the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    Success = 0,
    Error = 1,
    Usage = 2,
    DaemonDown = 10,
    Auth = 11,
    Conflict = 12,
    Network = 13,
}

impl ExitCode {
    pub fn code(self) -> i32 {
        self as i32
    }

    /// Map an anyhow error to an exit code by inspecting the error chain.
    pub fn from_error(err: &anyhow::Error) -> Self {
        // Walk the error chain for typed errors we recognize.
        for cause in err.chain() {
            if let Some(rpc_err) = cause.downcast_ref::<RpcError>() {
                return Self::from_rpc_code(rpc_err.code.as_str());
            }
            if let Some(io_err) = cause.downcast_ref::<std::io::Error>() {
                return match io_err.kind() {
                    std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::NotFound => Self::DaemonDown,
                    std::io::ErrorKind::TimedOut => Self::Network,
                    _ => Self::Error,
                };
            }
        }

        // Check the display string for common patterns.
        let msg = format!("{err:#}");
        if msg.contains("daemon") && (msg.contains("connect") || msg.contains("socket")) {
            return Self::DaemonDown;
        }
        if msg.contains("auth") || msg.contains("token") || msg.contains("unauthorized") {
            return Self::Auth;
        }

        Self::Error
    }

    /// Map an RPC error code string to an exit code.
    pub fn from_rpc_code(code: &str) -> Self {
        match code {
            "AUTH_INVALID_TOKEN" | "AUTH_FORBIDDEN" | "AUTH_TOKEN_REVOKED"
            | "AUTH_STATE_MISMATCH" | "AUTH_CODE_INVALID" => Self::Auth,

            "EDIT_PRECONDITION_FAILED" | "DOC_PATH_CONFLICT" => Self::Conflict,

            "VALIDATION_FAILED" | "PRECONDITION_REQUIRED" => Self::Usage,

            _ => Self::Error,
        }
    }

    /// Exit the process with this code.
    pub fn exit(self) -> ! {
        process::exit(self.code())
    }
}

impl From<ExitCode> for process::ExitCode {
    fn from(code: ExitCode) -> Self {
        process::ExitCode::from(code.code() as u8)
    }
}

/// A typed RPC error that can be embedded in an `anyhow::Error` chain.
#[derive(Debug)]
pub struct RpcError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RPC error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for RpcError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_values() {
        assert_eq!(ExitCode::Success.code(), 0);
        assert_eq!(ExitCode::Error.code(), 1);
        assert_eq!(ExitCode::Usage.code(), 2);
        assert_eq!(ExitCode::DaemonDown.code(), 10);
        assert_eq!(ExitCode::Auth.code(), 11);
        assert_eq!(ExitCode::Conflict.code(), 12);
        assert_eq!(ExitCode::Network.code(), 13);
    }

    #[test]
    fn from_rpc_code_auth_errors() {
        assert_eq!(ExitCode::from_rpc_code("AUTH_INVALID_TOKEN"), ExitCode::Auth);
        assert_eq!(ExitCode::from_rpc_code("AUTH_FORBIDDEN"), ExitCode::Auth);
        assert_eq!(ExitCode::from_rpc_code("AUTH_TOKEN_REVOKED"), ExitCode::Auth);
    }

    #[test]
    fn from_rpc_code_conflict_errors() {
        assert_eq!(ExitCode::from_rpc_code("EDIT_PRECONDITION_FAILED"), ExitCode::Conflict);
        assert_eq!(ExitCode::from_rpc_code("DOC_PATH_CONFLICT"), ExitCode::Conflict);
    }

    #[test]
    fn from_rpc_code_usage_errors() {
        assert_eq!(ExitCode::from_rpc_code("VALIDATION_FAILED"), ExitCode::Usage);
        assert_eq!(ExitCode::from_rpc_code("PRECONDITION_REQUIRED"), ExitCode::Usage);
    }

    #[test]
    fn from_rpc_code_unknown_is_general_error() {
        assert_eq!(ExitCode::from_rpc_code("UNKNOWN_CODE"), ExitCode::Error);
        assert_eq!(ExitCode::from_rpc_code("INTERNAL_ERROR"), ExitCode::Error);
    }

    #[test]
    fn from_error_connection_refused_is_daemon_down() {
        let err = anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "connection refused",
        ));
        assert_eq!(ExitCode::from_error(&err), ExitCode::DaemonDown);
    }

    #[test]
    fn from_error_timeout_is_network() {
        let err = anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "connection timed out",
        ));
        assert_eq!(ExitCode::from_error(&err), ExitCode::Network);
    }

    #[test]
    fn from_error_rpc_in_chain() {
        let rpc_err = RpcError {
            code: "AUTH_FORBIDDEN".into(),
            message: "you shall not pass".into(),
        };
        let err = anyhow::Error::new(rpc_err);
        assert_eq!(ExitCode::from_error(&err), ExitCode::Auth);
    }

    #[test]
    fn from_error_generic_is_error() {
        let err = anyhow::anyhow!("something went wrong");
        assert_eq!(ExitCode::from_error(&err), ExitCode::Error);
    }

    #[test]
    fn exit_code_to_process_exit_code() {
        let code: process::ExitCode = ExitCode::Success.into();
        // process::ExitCode doesn't expose the inner value, but we can test the conversion compiles.
        let _ = code;
    }
}
