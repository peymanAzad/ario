/// The result of parsing a command line, before starting either application.
#[derive(Debug, PartialEq, Eq)]
pub enum Command<T> {
    Run(T),
    Help,
    Version,
}

impl<T> Command<T> {
    pub fn from_common_arg(arg: &str) -> Option<Self> {
        match arg {
            "-h" | "--help" => Some(Self::Help),
            "-v" | "--version" => Some(Self::Version),
            _ => None,
        }
    }

    /// Print informational output, or return the options needed to start up.
    pub fn into_options(self, binary: &str, help: &str) -> Option<T> {
        match self {
            Self::Run(options) => Some(options),
            Self::Help => {
                println!("{binary} {}\n\n{help}", env!("CARGO_PKG_VERSION"));
                None
            }
            Self::Version => {
                println!("{binary} {}", env!("CARGO_PKG_VERSION"));
                None
            }
        }
    }
}
