// Dedup demo fixture: Type 3 near-miss duplicate.
// This is intentionally close to cli/result.rs, with small edits that make it a modified clone.

use std::process::{ExitCode, Termination};

#[derive(Debug)]
pub enum DemoCliOutcome {
    // Success
    Idle,
    FormatCompleted,
    // Warning error
    InvalidConfig,
    FormattingDiff,
    // Fatal error
    MissingFiles,
    FormatterCrashed,
}

impl DemoCliOutcome {
    pub fn code(&self) -> u8 {
        match self {
            Self::Idle | Self::FormatCompleted => 0,
            Self::InvalidConfig | Self::FormattingDiff => 1,
            Self::MissingFiles | Self::FormatterCrashed => 2,
        }
    }
}

impl Termination for DemoCliOutcome {
    fn report(self) -> ExitCode {
        ExitCode::from(self.code())
    }
}
