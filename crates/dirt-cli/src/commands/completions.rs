use std::io::{self, Write};
use std::path::Path;

use clap::CommandFactory;
use clap_complete::aot::Generator;
use clap_complete::{generate, shells};

use crate::cli::{Cli, CompletionShell};
use crate::error::CliError;

pub fn run_completions(shell: CompletionShell, output_path: Option<&Path>) -> Result<(), CliError> {
    let mut command = Cli::command();
    let mut buffer = Vec::new();
    match shell {
        CompletionShell::Bash => generate_for_shell(shells::Bash, &mut command, &mut buffer),
        CompletionShell::Zsh => generate_for_shell(shells::Zsh, &mut command, &mut buffer),
        CompletionShell::Fish => generate_for_shell(shells::Fish, &mut command, &mut buffer),
    }

    if let Some(path) = output_path {
        std::fs::write(path, &buffer)?;
        println!("{}", path.display());
    } else {
        io::stdout().write_all(&buffer)?;
    }

    Ok(())
}

fn generate_for_shell<G: Generator>(
    generator: G,
    command: &mut clap::Command,
    buffer: &mut Vec<u8>,
) {
    generate(generator, command, "dirt", buffer);
}
