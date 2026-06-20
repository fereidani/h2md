#![deny(warnings)]

use std::{
    fs,
    io::{self, Read},
};

use clap::Parser;

/// Convert HTML to Markdown
#[derive(Parser)]
#[command(name = "h2md")]
struct Cli {
    /// Input HTML file (reads from stdin if not provided)
    input: Option<String>,
    /// Output file (writes to stdout if not provided)
    #[arg(short = 'o')]
    output: Option<String>,
}

fn main() -> Result<(), h2md::Error> {
    let cli = Cli::parse();

    let html: Vec<u8> = if let Some(ref path) = cli.input {
        fs::read(path)?
    } else {
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf)?;
        buf
    };

    if html.is_empty() {
        return Ok(());
    }

    if let Some(ref path) = cli.output {
        let mut file = fs::File::create(path)?;
        h2md::convert(&html, &mut file)?;
    } else {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        h2md::convert(&html, &mut handle)?;
    }

    Ok(())
}
