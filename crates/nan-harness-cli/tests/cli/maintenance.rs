#[path = "maintenance/diagnostics.rs"]
mod diagnostics;
#[path = "maintenance/doctor.rs"]
mod doctor;
#[path = "maintenance/system_doctor.rs"]
mod system_doctor;
#[path = "maintenance/uninstall.rs"]
mod uninstall;

#[path = "maintenance/gentle_shell.rs"]
mod gentle_shell;

#[cfg(unix)]
#[path = "maintenance/gentle_shell_pty.rs"]
mod gentle_shell_pty;
