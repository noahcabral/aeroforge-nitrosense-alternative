use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Absolute path of a Windows system tool. `Command::new("reg")` lets CreateProcess
/// search the application directory first, which is user-writable in a portable install.
pub fn system32_path(tool: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    let file_name = Path::new(tool)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(tool);
    // Windows PowerShell is the one tool that does not live directly in System32.
    let relative = if file_name.eq_ignore_ascii_case("powershell") {
        r"WindowsPowerShell\v1.0\powershell.exe".to_string()
    } else {
        format!("{file_name}.exe")
    };
    PathBuf::from(root).join("System32").join(relative)
}

pub fn hidden_command(program: &str) -> Command {
    let mut command = Command::new(system32_path(program));
    apply_hidden_window(&mut command);
    command
}

pub fn hidden_powershell() -> Command {
    let mut command = hidden_command("powershell.exe");
    command.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-WindowStyle",
        "Hidden",
    ]);
    command
}

#[cfg(windows)]
fn apply_hidden_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn apply_hidden_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    #[test]
    fn system32_path_resolves_tools_under_the_windows_directory() {
        let reg = super::system32_path("reg");
        assert!(reg.ends_with(r"System32\reg.exe"), "{}", reg.display());
        assert_eq!(super::system32_path("sc.exe"), super::system32_path("sc"));
        let powershell = super::system32_path("powershell");
        assert!(
            powershell.ends_with(r"WindowsPowerShell\v1.0\powershell.exe"),
            "{}",
            powershell.display()
        );
        assert_eq!(super::hidden_command("reg").get_program(), reg.as_os_str());
    }

    #[test]
    fn hidden_powershell_includes_hidden_window_style() {
        let command = super::hidden_powershell();
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();

        assert!(args.iter().any(|arg| arg == "-WindowStyle"));
        assert!(args.iter().any(|arg| arg == "Hidden"));
    }
}
