//! `generate-dist` implementation: emit shell completions and the man page.
//!
//! The clap definition in `cli.rs` is the single source of truth; distro
//! packages (deb/rpm/Arch), the generic tarball, AppImage and Flatpak all
//! consume what this module writes so generated files never drift from the
//! real CLI surface.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use clap::CommandFactory;
use clap_complete::{Shell, generate_to};

use crate::cli::Cli;

const SHELLS: [(Shell, &str); 5] = [
    (Shell::Bash, "bash"),
    (Shell::Zsh, "zsh"),
    (Shell::Fish, "fish"),
    (Shell::PowerShell, "powershell"),
    (Shell::Elvish, "elvish"),
];

pub(crate) fn run_generate_dist(out_dir: &Path) -> Result<()> {
    let completions_dir = out_dir.join("completions");
    let man_dir = out_dir.join("man").join("man1");
    fs::create_dir_all(&completions_dir)
        .with_context(|| format!("creating {}", completions_dir.display()))?;
    fs::create_dir_all(&man_dir).with_context(|| format!("creating {}", man_dir.display()))?;

    let mut cmd = Cli::command();
    for (shell, name) in SHELLS {
        generate_to(shell, &mut cmd, "usage-monitor-cli", &completions_dir).with_context(|| {
            format!(
                "generating {name} completions into {}",
                completions_dir.display()
            )
        })?;
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
