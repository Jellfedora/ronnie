//! Windows: the shell's drag and drop (SHDoDragDrop, which draws the icons), of the data object the
//! shell makes for the files, as the Explorer's own.

use std::path::PathBuf;

use windows::core::HSTRING;
use windows::Win32::System::Com::{IBindCtx, IDataObject};
use windows::Win32::System::Ole::DROPEFFECT_COPY;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetActiveWindow, GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{BHID_DataObject, ILFree, SHCreateShellItemArrayFromIDLists, SHDoDragDrop, SHParseDisplayName};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON};

pub fn button_down() -> bool {
    // SAFETY: plain queries of the input state.
    unsafe {
        let key = if GetSystemMetrics(SM_SWAPBUTTON) != 0 { VK_RBUTTON } else { VK_LBUTTON };
        GetAsyncKeyState(key.0.into()) as u16 & 0x8000 != 0
    }
}

pub fn start(paths: &[PathBuf]) -> bool {
    match drag(paths) {
        Ok(()) => true,
        Err(e) => {
            crate::log::error(&format!("drag out: {e}"));
            false
        }
    }
}

/// Runs the drag (SHDoDragDrop returns once the files are dropped, or the drag given up).
fn drag(paths: &[PathBuf]) -> windows::core::Result<()> {
    let mut pidls: Vec<*const ITEMIDLIST> = Vec::new();
    let result = (|| {
        for path in paths {
            let mut pidl = std::ptr::null_mut();
            // SAFETY: `pidl` receives an item list, freed below.
            unsafe { SHParseDisplayName(&HSTRING::from(path.as_os_str()), None::<&IBindCtx>, &mut pidl, 0, None)? };
            pidls.push(pidl);
        }
        // SAFETY: the item lists are valid until freed below; the drag runs on this (the UI) thread,
        // where OLE is initialized (winit does it, for files dropped on the window).
        unsafe {
            let items = SHCreateShellItemArrayFromIDLists(&pidls)?;
            let data: IDataObject = items.BindToHandler(None::<&IBindCtx>, &BHID_DataObject)?;
            let window = GetActiveWindow();
            let result = SHDoDragDrop((!window.is_invalid()).then_some(window), &data, None, DROPEFFECT_COPY);
            // The button was let go during the drag: the window didn't hear it.
            super::took_button();
            result.map(|_| ())
        }
    })();
    for pidl in pidls {
        // SAFETY: allocated by SHParseDisplayName, freed once.
        unsafe { ILFree(Some(pidl)) };
    }
    result
}
