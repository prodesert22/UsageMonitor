use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use include_dir::{Dir, include_dir};

use crate::cli::WidgetInstallTarget;

const KDE_ID: &str = "dev.usage-monitor.kde";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const TARGETS: [&str; 2] = ["kde", "waybar"];
static KDE_PACKAGE: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/assets/kde/package");
static WAYBAR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/assets/waybar");

pub(crate) fn install(target: WidgetInstallTarget, force: bool) -> Result<()> {
    match target {
        WidgetInstallTarget::Kde => install_kde_stamped(force)?,
        WidgetInstallTarget::Waybar => install_waybar_stamped(force)?,
        WidgetInstallTarget::All => {
            install_kde_stamped(force)?;
            install_waybar_stamped(force)?;
        }
    }
    ensure_autostart()?;
    Ok(())
}

pub(crate) fn uninstall(target: WidgetInstallTarget) -> Result<()> {
    match target {
        WidgetInstallTarget::Kde => {
            uninstall_kde()?;
            remove_stamp("kde")?;
        }
        WidgetInstallTarget::Waybar => {
            uninstall_waybar()?;
            remove_stamp("waybar")?;
        }
        WidgetInstallTarget::All => {
            uninstall_kde()?;
            remove_stamp("kde")?;
            uninstall_waybar()?;
            remove_stamp("waybar")?;
        }
    }
    // Drop the login autostart once no widget remains installed.
    if !any_widget_installed()? {
        remove_file_if_exists(&autostart_path()?)?;
    }
    Ok(())
}

/// Reinstall any already-installed widget whose recorded version is older than
/// this binary. Invoked from the login autostart entry so a CLI upgrade
/// propagates to the widgets without the user re-running `widget install`.
pub(crate) fn sync() -> Result<()> {
    let mut upgraded = false;
    for target in TARGETS {
        let stamp = read_stamp(target)?;
        if !is_stale(stamp.as_deref()) {
            continue; // never installed, or already current
        }
        let stamp = stamp.expect("is_stale only true when a stamp exists");
        println!("Upgrading {target} widget {stamp} -> {VERSION}");
        match target {
            "kde" => install_kde_stamped(true)?,
            "waybar" => install_waybar_stamped(true)?,
            _ => unreachable!(),
        }
        upgraded = true;
    }
    if upgraded {
        ensure_autostart()?;
    } else {
        println!("Widgets are up to date ({VERSION})");
    }
    Ok(())
}

fn install_kde_stamped(force: bool) -> Result<()> {
    install_kde(force)?;
    write_stamp("kde")
}

fn install_waybar_stamped(force: bool) -> Result<()> {
    install_waybar(force)?;
    write_stamp("waybar")
}

pub(crate) fn doctor() -> Result<()> {
    println!("usage-monitor-cli: {}", std::env::current_exe()?.display());
    println!("version: {VERSION}");
    println!("data home: {}", data_home()?.display());
    println!("bin dir: {}", local_bin()?.display());
    println!(
        "kpackagetool6: {}",
        find_command("kpackagetool6").map_or("missing".to_string(), |p| p.display().to_string())
    );
    println!("KDE plasmoid: {}", kde_plasmoid_dir()?.display());
    println!("Waybar wrapper: {}", waybar_bin_path()?.display());
    for target in TARGETS {
        println!(
            "{target} installed version: {}",
            read_stamp(target)?.unwrap_or_else(|| "not installed".to_string())
        );
    }
    println!("autostart entry: {}", autostart_path()?.display());
    Ok(())
}

fn install_kde(force: bool) -> Result<()> {
    let stage = data_home()?.join("usage-monitor/kde/package");
    write_dir(&KDE_PACKAGE, &stage, true)?;

    if let Some(tool) = find_command("kpackagetool6") {
        let mode = if kde_plasmoid_dir()?.exists() {
            "--upgrade"
        } else {
            "--install"
        };
        let output = Command::new(&tool)
            .args(["--type", "Plasma/Applet", mode])
            .arg(&stage)
            .output()
            .with_context(|| format!("failed to run {}", tool.display()))?;
        if output.status.success() {
            println!("KDE widget installed with {}", tool.display());
            return Ok(());
        }
        if !force {
            anyhow::bail!(
                "kpackagetool6 failed: {}\nRetry with `usage-monitor-cli widget install kde --force` to use direct copy fallback.",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
    }

    let target = kde_plasmoid_dir()?;
    write_dir(&KDE_PACKAGE, &target, true)?;
    println!("KDE widget installed at {}", target.display());
    println!("Restart Plasma or re-add the widget if the old UI is cached.");
    Ok(())
}

fn install_waybar(_force: bool) -> Result<()> {
    let dir = data_home()?.join("usage-monitor/waybar");
    write_dir(&WAYBAR, &dir, true)?;

    let bin_dir = local_bin()?;
    fs::create_dir_all(&bin_dir).with_context(|| format!("create {}", bin_dir.display()))?;
    let bin = waybar_bin_path()?;
    let script = dir.join("usage-monitor-waybar");
    replace_symlink(&script, &bin)?;

    println!("Waybar wrapper installed at {}", bin.display());
    println!("Waybar setup is two edits in ~/.config/waybar/config.jsonc:");
    println!("1) Define the module:");
    println!(
        r#"   "custom/usage-monitor": {{
     "exec": "{}",
     "return-type": "json",
     "interval": 30,
     "format": "{{text}}",
     "tooltip": true
   }}"#,
        bin.display()
    );
    println!("2) Add its name to a bar so it renders (it is ignored otherwise):");
    println!(r#"   "modules-right": ["...", "custom/usage-monitor", "clock"]"#);
    println!("Then reload: killall -SIGUSR2 waybar");
    Ok(())
}

fn uninstall_kde() -> Result<()> {
    if let Some(tool) = find_command("kpackagetool6") {
        let _ = Command::new(&tool)
            .args(["--type", "Plasma/Applet", "--remove", KDE_ID])
            .status();
    }
    remove_dir_if_exists(&kde_plasmoid_dir()?)?;
    remove_dir_if_exists(&data_home()?.join("usage-monitor/kde"))?;
    println!("KDE widget removed");
    Ok(())
}

fn uninstall_waybar() -> Result<()> {
    remove_file_if_exists(&waybar_bin_path()?)?;
    remove_dir_if_exists(&data_home()?.join("usage-monitor/waybar"))?;
    println!("Waybar wrapper removed");
    Ok(())
}

fn write_dir(dir: &Dir<'_>, dest: &Path, overwrite: bool) -> Result<()> {
    if dest.exists() && overwrite {
        fs::remove_dir_all(dest).with_context(|| format!("remove {}", dest.display()))?;
    }
    fs::create_dir_all(dest).with_context(|| format!("create {}", dest.display()))?;
    for file in dir.files() {
        if is_python_cache(file.path()) {
            continue;
        }
        let path = dest.join(relative_to_dir(dir, file.path()));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        fs::write(&path, file.contents()).with_context(|| format!("write {}", path.display()))?;
        set_executable_if_needed(&path)?;
    }
    for child in dir.dirs() {
        if is_python_cache(child.path()) {
            continue;
        }
        write_dir(child, &dest.join(relative_to_dir(dir, child.path())), false)?;
    }
    Ok(())
}

/// Skip Python bytecode caches that may be embedded from the asset tree.
fn is_python_cache(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str() == OsStr::new("__pycache__"))
        || path.extension() == Some(OsStr::new("pyc"))
}

fn relative_to_dir<'a>(dir: &Dir<'_>, path: &'a Path) -> &'a Path {
    path.strip_prefix(dir.path()).unwrap_or(path)
}

fn data_home() -> Result<PathBuf> {
    if let Some(value) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(value));
    }
    Ok(home_dir()?.join(".local/share"))
}

fn local_bin() -> Result<PathBuf> {
    Ok(home_dir()?.join(".local/bin"))
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("HOME is not set"))
}

fn kde_plasmoid_dir() -> Result<PathBuf> {
    Ok(data_home()?.join(format!("plasma/plasmoids/{KDE_ID}")))
}

fn waybar_bin_path() -> Result<PathBuf> {
    Ok(local_bin()?.join("usage-monitor-waybar"))
}

fn config_home() -> Result<PathBuf> {
    if let Some(value) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(value));
    }
    Ok(home_dir()?.join(".config"))
}

fn stamp_path(target: &str) -> Result<PathBuf> {
    Ok(data_home()?.join(format!("usage-monitor/{target}.version")))
}

fn write_stamp(target: &str) -> Result<()> {
    let path = stamp_path(target)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(&path, VERSION).with_context(|| format!("write {}", path.display()))
}

fn read_stamp(target: &str) -> Result<Option<String>> {
    match fs::read_to_string(stamp_path(target)?) {
        Ok(value) => Ok(Some(value.trim().to_string())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("read {target} version stamp")),
    }
}

fn remove_stamp(target: &str) -> Result<()> {
    remove_file_if_exists(&stamp_path(target)?)
}

fn any_widget_installed() -> Result<bool> {
    for target in TARGETS {
        if read_stamp(target)?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// A widget needs a sync upgrade when it is installed (has a stamp) and that
/// recorded version differs from this binary's. A missing stamp means the widget
/// was never installed, so `sync` leaves it alone.
fn is_stale(stamp: Option<&str>) -> bool {
    matches!(stamp, Some(version) if version != VERSION)
}

fn autostart_path() -> Result<PathBuf> {
    Ok(config_home()?.join("autostart/usage-monitor-widget-sync.desktop"))
}

/// Write (or refresh) the XDG autostart entry that runs `widget sync` at login,
/// so a CLI upgrade is applied to the installed widgets on the next session.
fn ensure_autostart() -> Result<()> {
    let exe = std::env::current_exe().context("resolve current executable")?;
    let path = autostart_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(&path, autostart_entry(&exe)).with_context(|| format!("write {}", path.display()))
}

fn autostart_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Usage Monitor widget sync\n\
         Comment=Upgrade installed Usage Monitor widgets to match the CLI version\n\
         Exec=\"{}\" widget sync\n\
         X-GNOME-Autostart-enabled=true\n\
         NoDisplay=true\n",
        exe.display()
    )
}

fn find_command(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn remove_dir_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    if path.exists() || path.is_symlink() {
        fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn replace_symlink(src: &Path, dest: &Path) -> Result<()> {
    remove_file_if_exists(dest)?;
    std::os::unix::fs::symlink(src, dest)
        .with_context(|| format!("symlink {} -> {}", dest.display(), src.display()))
}

#[cfg(not(unix))]
fn replace_symlink(src: &Path, dest: &Path) -> Result<()> {
    fs::copy(src, dest).with_context(|| format!("copy {} to {}", src.display(), dest.display()))?;
    Ok(())
}

#[cfg(unix)]
fn set_executable_if_needed(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let executable = path.file_name() == Some(OsStr::new("usage-monitor-waybar"))
        || path.extension() == Some(OsStr::new("py"));
    if executable {
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_executable_if_needed(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_python_cache_matches_pyc_and_pycache_dirs() {
        assert!(is_python_cache(Path::new("__pycache__")));
        assert!(is_python_cache(Path::new("contents/code/__pycache__")));
        assert!(is_python_cache(Path::new(
            "contents/code/__pycache__/widget.cpython-314.pyc"
        )));
        assert!(is_python_cache(Path::new("code/module.pyc")));
        assert!(!is_python_cache(Path::new("contents/code/widget.py")));
        assert!(!is_python_cache(Path::new("metadata.json")));
    }

    #[test]
    fn write_dir_materializes_waybar_assets() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("waybar");
        write_dir(&WAYBAR, &dest, true).unwrap();

        assert!(dest.join("usage-monitor-waybar").is_file());
        assert!(dest.join("usage_monitor_waybar.py").is_file());
        assert!(no_python_cache(&dest));
    }

    #[test]
    fn write_dir_skips_embedded_python_caches() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("kde");
        write_dir(&KDE_PACKAGE, &dest, true).unwrap();

        assert!(dest.join("metadata.json").is_file());
        assert!(
            no_python_cache(&dest),
            "install tree must not contain Python bytecode caches"
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_dir_marks_wrapper_and_python_executable() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("waybar");
        write_dir(&WAYBAR, &dest, true).unwrap();

        for name in ["usage-monitor-waybar", "usage_monitor_waybar.py"] {
            let mode = fs::metadata(dest.join(name)).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "{name} should be executable");
        }
    }

    #[test]
    fn write_dir_overwrite_clears_stale_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("waybar");
        fs::create_dir_all(&dest).unwrap();
        let stale = dest.join("stale.txt");
        fs::write(&stale, b"old").unwrap();

        write_dir(&WAYBAR, &dest, true).unwrap();

        assert!(!stale.exists(), "overwrite should remove stale files");
        assert!(dest.join("usage-monitor-waybar").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn replace_symlink_repoints_existing_link() {
        let tmp = tempfile::tempdir().unwrap();
        let first = tmp.path().join("first");
        let second = tmp.path().join("second");
        let link = tmp.path().join("link");
        fs::write(&first, b"1").unwrap();
        fs::write(&second, b"2").unwrap();

        replace_symlink(&first, &link).unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), first);

        replace_symlink(&second, &link).unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), second);
    }

    #[test]
    fn is_stale_only_for_installed_outdated_versions() {
        assert!(!is_stale(None), "missing stamp must not trigger a sync");
        assert!(!is_stale(Some(VERSION)), "current version is not stale");
        assert!(is_stale(Some("0.0.1")), "older recorded version is stale");
    }

    #[test]
    fn autostart_entry_launches_widget_sync() {
        let entry = autostart_entry(Path::new("/opt/um/usage-monitor-cli"));
        assert!(entry.contains("Type=Application"));
        assert!(entry.contains("Exec=\"/opt/um/usage-monitor-cli\" widget sync"));
        assert!(entry.contains("NoDisplay=true"));
    }

    #[test]
    fn stamp_write_read_remove_roundtrip() {
        with_temp_home(|_| {
            assert_eq!(read_stamp("waybar").unwrap(), None);
            write_stamp("waybar").unwrap();
            assert_eq!(read_stamp("waybar").unwrap().as_deref(), Some(VERSION));
            remove_stamp("waybar").unwrap();
            assert_eq!(read_stamp("waybar").unwrap(), None);
        });
    }

    #[cfg(unix)]
    #[test]
    fn install_waybar_records_stamp_and_autostart() {
        with_temp_home(|_| {
            install(WidgetInstallTarget::Waybar, false).unwrap();
            assert_eq!(read_stamp("waybar").unwrap().as_deref(), Some(VERSION));
            assert!(waybar_bin_path().unwrap().is_symlink());
            assert!(autostart_path().unwrap().is_file());
        });
    }

    #[cfg(unix)]
    #[test]
    fn sync_upgrades_stale_and_is_noop_when_absent_or_current() {
        with_temp_home(|_| {
            // Nothing installed: sync must not create or install anything.
            sync().unwrap();
            assert_eq!(read_stamp("waybar").unwrap(), None);

            install(WidgetInstallTarget::Waybar, false).unwrap();
            // Already current: sync leaves the stamp untouched.
            sync().unwrap();
            assert_eq!(read_stamp("waybar").unwrap().as_deref(), Some(VERSION));

            // Simulate an older install, then sync should bump it back.
            fs::write(stamp_path("waybar").unwrap(), "0.0.1").unwrap();
            sync().unwrap();
            assert_eq!(read_stamp("waybar").unwrap().as_deref(), Some(VERSION));
        });
    }

    #[cfg(unix)]
    #[test]
    fn uninstall_clears_stamp_and_autostart_when_last_widget() {
        with_temp_home(|_| {
            install(WidgetInstallTarget::Waybar, false).unwrap();
            assert!(autostart_path().unwrap().is_file());

            uninstall(WidgetInstallTarget::Waybar).unwrap();
            assert_eq!(read_stamp("waybar").unwrap(), None);
            assert!(
                !autostart_path().unwrap().exists(),
                "autostart entry should be removed with the last widget"
            );
        });
    }

    fn no_python_cache(root: &Path) -> bool {
        fn walk(dir: &Path) -> bool {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if is_python_cache(&path) {
                    return false;
                }
                if path.is_dir() && !walk(&path) {
                    return false;
                }
            }
            true
        }
        walk(root)
    }

    /// Serialize env-mutating tests and point HOME/XDG dirs at a throwaway
    /// tempdir so the widget install paths resolve inside it. The lock keeps the
    /// process-global env consistent across parallel test threads.
    fn with_temp_home<R>(f: impl FnOnce(&Path) -> R) -> R {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let tmp = tempfile::tempdir().unwrap();
        let keys = ["HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME"];
        let saved: Vec<(&str, Option<std::ffi::OsString>)> =
            keys.iter().map(|k| (*k, std::env::var_os(k))).collect();

        // SAFETY: access to the process environment is serialized by ENV_LOCK,
        // and no other test reads these variables without holding the lock.
        unsafe {
            std::env::set_var("HOME", tmp.path());
            std::env::set_var("XDG_DATA_HOME", tmp.path().join("share"));
            std::env::set_var("XDG_CONFIG_HOME", tmp.path().join("config"));
        }

        let result = f(tmp.path());

        unsafe {
            for (key, value) in saved {
                match value {
                    Some(val) => std::env::set_var(key, val),
                    None => std::env::remove_var(key),
                }
            }
        }
        result
    }
}
