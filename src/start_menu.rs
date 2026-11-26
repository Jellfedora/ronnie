//! Windows: a « Ronnie » shortcut in the Start menu. Ronnie comes as a zip with no installer, so
//! without it Windows Search only finds the file, ronnie.exe, spelled in lowercase. Made again at each
//! launch of an official build, so it follows the exe when it is moved.

pub fn ensure() {
    if !crate::config::OFFICIAL {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("start-menu".into())
        .spawn(|| {
            if let Err(e) = make() {
                crate::log::info(&format!("start menu shortcut: {e}"));
            }
        });
}

fn make() -> windows::core::Result<()> {
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        IPersistFile,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{HSTRING, Interface};

    let (Ok(exe), Some(appdata)) = (std::env::current_exe(), std::env::var_os("APPDATA")) else {
        return Ok(());
    };
    let programs = std::path::PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs");
    if !programs.is_dir() {
        return Ok(());
    }
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(exe.as_path()))?;
        if let Some(dir) = exe.parent() {
            link.SetWorkingDirectory(&HSTRING::from(dir))?;
        }
        link.SetDescription(&HSTRING::from("Ronnie"))?;
        link.SetIconLocation(&HSTRING::from(exe.as_path()), 0)?;
        link.cast::<IPersistFile>()?
            .Save(&HSTRING::from(programs.join("Ronnie.lnk").as_path()), true)
    }
}
