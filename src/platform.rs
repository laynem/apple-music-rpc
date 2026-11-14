//! The few raw Win32 calls the app needs, with inert stand-ins so the rest of
//! the code can be type-checked on other systems.

#[cfg(windows)]
mod imp {
    use windows::core::{w, HSTRING};
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::SystemInformation::GetLocalTime;
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, MessageBoxW, SetTimer, TranslateMessage, IDOK, MB_ICONERROR,
        MB_ICONINFORMATION, MB_OK, MB_OKCANCEL, MSG,
    };

    /// False when another copy is already running.
    pub fn single_instance() -> bool {
        unsafe {
            // Same name the earlier script version used. The handle is kept for the process lifetime.
            let _mutex = CreateMutexW(None, false, w!("AppleMusicDiscordRPC"));
            GetLastError() != ERROR_ALREADY_EXISTS
        }
    }

    /// OK/Cancel prompt; true when OK was clicked.
    pub fn confirm(title: &str, text: &str) -> bool {
        unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(title), MB_OKCANCEL | MB_ICONINFORMATION) == IDOK }
    }

    pub fn info_box(title: &str, text: &str) {
        unsafe {
            MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(title), MB_OK | MB_ICONINFORMATION);
        }
    }

    pub fn error_box(title: &str, text: &str) {
        unsafe {
            MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(title), MB_OK | MB_ICONERROR);
        }
    }

    /// "YYYY-MM-DD HH:MM:SS" in local time.
    pub fn timestamp() -> String {
        let t = unsafe { GetLocalTime() };
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
    }

    /// Pumps window messages (the tray icon needs this), calling `tick` after
    /// each one and at least twice a second, until `tick` returns false.
    pub fn run_loop(mut tick: impl FnMut() -> bool) {
        unsafe {
            SetTimer(None, 0, 500, None);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
                if !tick() {
                    break;
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn single_instance() -> bool {
        true
    }
    pub fn confirm(_title: &str, _text: &str) -> bool {
        false
    }
    pub fn info_box(_title: &str, _text: &str) {}
    pub fn error_box(_title: &str, _text: &str) {}
    pub fn timestamp() -> String {
        String::new()
    }
    pub fn run_loop(mut tick: impl FnMut() -> bool) {
        while tick() {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
}

pub use imp::*;
