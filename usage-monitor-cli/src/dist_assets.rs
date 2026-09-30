//! `generate-dist` implementation: emit shell completions and the man page.
//!
//! The clap definition in `cli.rs` is the single source of truth; distro
//! packages (deb/rpm/Arch), the generic tarball, AppImage and Flatpak all
//! consume what this module writes so generated files never drift from the
//! real CLI surface.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use anyhow::{Context, Result};
use clap::{Command, CommandFactory, builder::PossibleValuesParser};
use clap_complete::{Shell, generate, generate_to};
use usage_monitor_cli::provider::registry::ProviderRegistry;

use crate::cli::Cli;

const SHELLS: [(Shell, &str); 5] = [
    (Shell::Bash, "bash"),
    (Shell::Zsh, "zsh"),
    (Shell::Fish, "fish"),
    (Shell::PowerShell, "powershell"),
    (Shell::Elvish, "elvish"),
];

pub(crate) fn print_completions(shell: Shell) -> Result<()> {
    write_completions(shell, &mut io::stdout())?;
    Ok(())
}

fn write_completions(shell: Shell, writer: &mut impl Write) -> Result<()> {
    let mut cmd = completion_command();
    generate(shell, &mut cmd, "usage-monitor-cli", &mut *writer);
    // clap_complete's Fish generator handles value options but omits positional
    // possible values. Add provider arguments explicitly for the same behavior.
    if shell == Shell::Fish {
        let mut providers: Vec<_> = ProviderRegistry::with_defaults()
            .all_metadata()
            .into_iter()
            .map(|meta| meta.id)
            .collect();
        providers.sort_unstable();
        writeln!(
            writer,
            "complete -c usage-monitor-cli -n '__fish_seen_subcommand_from enable disable auto fetch waybar kde gnome' -f -a '{}'",
            providers.join(" ")
        )?;
    }
    Ok(())
}

fn completion_command() -> Command {
    let providers: Vec<&'static str> = ProviderRegistry::with_defaults()
        .all_metadata()
        .into_iter()
        .map(|meta| meta.id)
        .collect();
    let mut cmd = Cli::command();
    for name in ["enable", "disable", "auto", "fetch"] {
        if let Some(sub) = cmd.find_subcommand_mut(name) {
            *sub = sub.clone().mut_arg("provider", |arg| {
                arg.value_parser(PossibleValuesParser::new(providers.clone()))
            });
        }
    }
    if let Some(widget) = cmd.find_subcommand_mut("widget") {
        for name in ["waybar", "kde", "gnome"] {
            if let Some(sub) = widget.find_subcommand_mut(name) {
                *sub = sub.clone().mut_arg("provider", |arg| {
                    arg.value_parser(PossibleValuesParser::new(providers.clone()))
                });
            }
        }
    }
    cmd
}

pub(crate) fn run_generate_dist(out_dir: &Path) -> Result<()> {
    let completions_dir = out_dir.join("completions");
    let man_dir = out_dir.join("man").join("man1");
    fs::create_dir_all(&completions_dir)
        .with_context(|| format!("creating {}", completions_dir.display()))?;
    fs::create_dir_all(&man_dir).with_context(|| format!("creating {}", man_dir.display()))?;

    let mut cmd = completion_command();
    for (shell, name) in SHELLS {
        generate_to(shell, &mut cmd, "usage-monitor-cli", &completions_dir).with_context(|| {
            format!(
                "generating {name} completions into {}",
                completions_dir.display()
            )
        })?;
        if shell == Shell::Fish {
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(completions_dir.join("usage-monitor-cli.fish"))?;
            let mut providers: Vec<_> = ProviderRegistry::with_defaults()
                .all_metadata()
                .into_iter()
                .map(|meta| meta.id)
                .collect();
            providers.sort_unstable();
            writeln!(
                file,
                "complete -c usage-monitor-cli -n '__fish_seen_subcommand_from enable disable auto fetch waybar kde gnome' -f -a '{}'",
                providers.join(" ")
            )?;
        }
    }

    let man = clap_mangen::Man::new(cmd);
    let mut buffer = Vec::new();
    man.render(&mut buffer).context("rendering man page")?;
    let man_path = man_dir.join("usage-monitor-cli.1");
    fs::write(&man_path, buffer).with_context(|| format!("writing {}", man_path.display()))?;
    let debian_dir = out_dir.join("debian");
    fs::create_dir_all(&debian_dir).context("creating Debian changelog directory")?;
    let changelog = format!(
        "usage-monitor-cli ({}-1) unstable; urgency=medium\n\n  * Package Usage Monitor {}.\n\n -- Pedro Antonio Trindade <pedroantonioppms@gmail.com>  {}\n",
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION"),
        chrono::Utc::now().format("%a, %d %b %Y %H:%M:%S %z")
    );
    fs::write(debian_dir.join("changelog"), changelog).context("writing Debian changelog")?;
    println!(
        "wrote completions, man page and Debian changelog to {}",
        out_dir.display()
    );
    Ok(())
}
