//! Windows processes: who started whom (the process list).

use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};

/// A running process: its id, its parent's, and its program's file name.
pub struct Process {
    pub pid: u32,
    pub parent: u32,
    pub exe: String,
}

/// The processes running now.
pub fn processes() -> Vec<Process> {
    let mut out = Vec::new();
    // SAFETY: plain calls on a snapshot handle we own and close; `entry` has its size set as required.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snapshot, &mut entry) != 0;
        while ok {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            out.push(Process { pid: entry.th32ProcessID, parent: entry.th32ParentProcessID, exe: String::from_utf16_lossy(&entry.szExeFile[..len]) });
            ok = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    out
}

pub fn parent_pid(pid: u32) -> Option<u32> {
    processes().into_iter().find(|p| p.pid == pid).map(|p| p.parent)
}

/// The program `shell` started and that runs in the foreground (`npm`, `vim`...), without ".exe": its
/// latest child, the console hosts left out.
pub fn foreground(shell: u32) -> Option<String> {
    let all = processes();
    let child = all.iter().rev().find(|p| p.parent == shell && !matches!(p.exe.to_ascii_lowercase().as_str(), "conhost.exe" | "openconsole.exe"))?;
    Some(child.exe.strip_suffix(".exe").or_else(|| child.exe.strip_suffix(".EXE")).unwrap_or(&child.exe).to_owned())
}
