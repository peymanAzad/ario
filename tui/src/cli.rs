use common::cli::Command;

use crate::icons::GlyphMode;

pub const HELP: &str = "A terminal download manager powered by aria2.

Usage: ario [OPTIONS]

Options:
      --glyphs <MODE>  Icon style: nerd, unicode, or ascii
  -h, --help           Print help
  -v, --version        Print version";

pub fn parse<I, S>(args: I) -> anyhow::Result<Command<Option<GlyphMode>>>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into);
    let mut mode = None;
    while let Some(arg) = args.next() {
        if let Some(command) = Command::from_common_arg(&arg) {
            return Ok(command);
        }
        let value = if arg == "--glyphs" {
            args.next()
                .ok_or_else(|| anyhow::anyhow!("--glyphs requires nerd, unicode, or ascii"))?
        } else if let Some(value) = arg.strip_prefix("--glyphs=") {
            value.to_string()
        } else {
            anyhow::bail!("unknown argument {arg:?}");
        };
        if mode.is_some() {
            anyhow::bail!("--glyphs may only be specified once");
        }
        mode = Some(value.parse().map_err(anyhow::Error::msg)?);
    }
    Ok(Command::Run(mode))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_cli_forms_and_rejects_bad_values() {
        assert_eq!(
            parse(["--glyphs=nerd"]).unwrap(),
            Command::Run(Some(GlyphMode::NerdFont))
        );
        assert_eq!(
            parse(["--glyphs", "ascii"]).unwrap(),
            Command::Run(Some(GlyphMode::Ascii))
        );
        assert!(parse(["--glyphs=emoji"]).is_err());
        assert!(parse(["--glyphs"]).is_err());
        assert!(parse(["--other"]).is_err());
    }
}
