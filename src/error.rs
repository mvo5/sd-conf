use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{}:{line}: {reason}", path.display())]
    Parse {
        path: PathBuf,
        line: u32,
        reason: &'static str,
    },
}
