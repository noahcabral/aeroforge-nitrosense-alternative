use std::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn hidden_command(program: &str) -> Command {
    let mut command = Command::new(program);
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
