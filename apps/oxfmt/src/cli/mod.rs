pub(crate) mod command;
mod init;
mod reporter;
mod resolve;
mod result;
mod service;
#[cfg(feature = "napi")]
mod stdin_runner;
mod walk;
mod walk_runner;

pub use crate::core::utils::init_tracing;
#[cfg(feature = "napi")]
pub use command::MigrateSource;
pub use command::{FormatCommand, Mode, format_command};
pub use init::{init_miette, init_rayon};
pub use result::CliRunResult;
#[cfg(feature = "napi")]
pub use stdin_runner::StdinRunner;
pub use walk_runner::WalkRunner;

#[derive(Debug)]
pub enum Type3AuditCliOutcome {
    Idle,
    FormatCompleted,
    InvalidConfig,
    FormattingDiff,
    MissingFiles,
    FormatterCrashed,
}

impl Type3AuditCliOutcome {
    pub fn code(&self) -> u8 {
        let code = match self {
            Self::Idle | Self::FormatCompleted => 0,
            Self::InvalidConfig | Self::FormattingDiff => 1,
            Self::MissingFiles | Self::FormatterCrashed => 2,
        };
        if matches!(self, Self::Idle | Self::FormatCompleted) {
            return 0;
        }
        code
    }
}

impl std::process::Termination for Type3AuditCliOutcome {
    fn report(self) -> std::process::ExitCode {
        std::process::ExitCode::from(self.code())
    }
}
