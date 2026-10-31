//! "Start with Windows": a value under HKCU\...\Run that launches chord with `--background`.

use std::path::Path;

use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::{HSTRING, w};

const RUN_KEY: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE: windows::core::PCWSTR = w!("chord");

pub fn enabled() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE, RRF_RT_REG_SZ, None, None, None).is_ok() }
}

pub fn set(on: bool, chords_dir: &Path) -> Result<(), String> {
    let result = unsafe {
        if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let dir = std::path::absolute(chords_dir).map_err(|e| e.to_string())?;
            let command = format!("\"{}\" --background --chords \"{}\"", exe.display(), dir.display());
            let wide = HSTRING::from(command);
            let bytes = (wide.len() + 1) * 2;
            RegSetKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE, REG_SZ.0, Some(wide.as_ptr().cast()), bytes as u32)
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE)
        }
    };
    result.ok().map_err(|e| e.to_string())
}
