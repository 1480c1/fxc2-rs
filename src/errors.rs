use std::{fmt, process::ExitCode};

/// Error conditions for exiting the program
#[derive(Debug, thiserror::Error)]
pub enum UsageError {
    /// To trigger: /AAAA
    #[error("Unknown or invalid option '{0}', use /? for help")]
    UnknownArgument(String),
    /// To trigger: /T
    #[error("'{0}' option requires a parameter, use /? to get usage information")]
    RequiresArg(String),
    /// To trigger: Blit.vs Blit.vs
    #[error("Too many files specified ('{0}' was the last one), use /? to get usage information")]
    TooManyFiles(String),
    /// To trigger: /?
    #[error("Check https://learn.microsoft.com/en-us/windows/win32/direct3dtools/dx-graphics-tools-fxc-syntax for usage information.")]
    HelpRequested,
}

impl From<UsageError> for ExitCode {
    fn from(err: UsageError) -> ExitCode {
        eprint!("{err}");
        ExitCode::FAILURE
    }
}
