use std::cell::RefCell;
use std::mem::size_of;
use std::ptr::copy_nonoverlapping;
use std::sync::Once;

use qrcode::{Color, QrCode};
use tiny_keccak::{Hasher, Keccak};
use windows::Win32::Foundation::{ERROR_SUCCESS, GlobalFree, GetLastError, HANDLE, HWND, LPARAM, LRESULT, SetLastError, WIN32_ERROR, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, Result, w};

use crate::locale::Language;
use crate::log;
use wintervibe::card::{self, Card, DonationContent, DonationHits};

const CLASS: PCWSTR = w!("WinterVibeDonation");
const WIDTH: f32 = 480.0;
const HEIGHT: f32 = 580.0;
const TIMER: usize = 1;
const ADDRESS: &str = "0xcC170d39cEE9786CdBe9E75a5586a3294191b91C";

thread_local! {
    static DONATION: RefCell<Option<Donation>> = const { RefCell::new(None) };
}

struct Donation {
    window: HWND,
    card: Card,
    hits: DonationHits,
    language: Language,
    light: bool,
    copied: bool,
    qr: Vec<bool>,
    qr_size: usize,
}

pub fn show(language: Language, light: bool) {
    if let Some(window) = DONATION.with_borrow(|donation| donation.as_ref().map(|donation| donation.window)) {
        DONATION.with_borrow_mut(|donation| {
            if let Some(donation) = donation.as_mut() {
                donation.language = language;
                donation.light = light;
            }
        });
        repaint(window, false);
        unsafe {
            let _ = ShowWindow(window, SW_SHOWNORMAL);
            let _ = SetForegroundWindow(window);
        }
        return;
    }
    if let Err(error) = open(language, light) {
        log!("donation window failed: {error}");
    }
}

fn open(language: Language, light: bool) -> Result<()> {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        let _ = card::register(CLASS, Some(procedure));
    });

    if !valid_evm_address(ADDRESS) {
        return Err(windows::core::Error::new(windows::core::HRESULT(0x80070057_u32 as i32), "Invalid EVM receive address"));
    }
    let code = QrCode::new(ADDRESS.as_bytes()).map_err(|error| windows::core::Error::new(windows::core::HRESULT(0x80004005_u32 as i32), error.to_string()))?;
    let qr_size = code.width();
    let qr: Vec<bool> = (0..qr_size * qr_size).map(|index| code[(index % qr_size, index / qr_size)] == Color::Dark).collect();
    let (scale, origin) = card::placement(WIDTH, HEIGHT);
    let card = Card::new(WIDTH, HEIGHT, scale)?;
    let content = content(language, light, false);
    let (pixels, hits) = card.paint_donation(&content, &qr, qr_size)?;
    unsafe {
        let window = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            CLASS,
            w!("WinterVibe"),
            WS_POPUP,
            origin.x,
            origin.y,
            card.size.0,
            card.size.1,
            None,
            None,
            None,
            None,
        )?;
        card::present(window, card.size, &pixels, Some(origin));
        DONATION.set(Some(Donation { window, card, hits, language, light, copied: false, qr, qr_size }));
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
    }
    Ok(())
}

fn content(language: Language, light: bool, copied_state: bool) -> DonationContent<'static> {
    let strings = language.strings();
    DonationContent {
        title: strings.support_author,
        subtitle: strings.donation_subtitle,
        tab: strings.evm_wallet,
        network_note: strings.evm_network_note,
        address: ADDRESS,
        copy: strings.copy_address,
        copied: strings.address_copied,
        locale: if language == Language::Russian { "ru-RU" } else { "en-US" },
        light,
        copied_state,
    }
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_NCHITTEST => {
            let local = card::point(lparam);
            let hit = DONATION.with_borrow(|donation| {
                let donation = donation.as_ref()?;
                let mut local = local;
                if !unsafe { ScreenToClient(window, &mut local) }.as_bool() {
                    return None;
                }
                let point = (local.x as f32 / donation.card.scale, local.y as f32 / donation.card.scale);
                Some(
                    card::contains(&donation.hits.address, point)
                        || card::contains(&donation.hits.copy, point)
                        || card::contains(&donation.hits.close, point),
                )
            });
            LRESULT(if hit == Some(true) { HTCLIENT as isize } else { HTCAPTION as isize })
        }
        WM_LBUTTONUP => {
            let point = DONATION.with_borrow(|donation| {
                let donation = donation.as_ref()?;
                let point = card::point(lparam);
                let point = (point.x as f32 / donation.card.scale, point.y as f32 / donation.card.scale);
                Some((
                    card::contains(&donation.hits.address, point) || card::contains(&donation.hits.copy, point),
                    card::contains(&donation.hits.close, point),
                ))
            });
            match point {
                Some((_, true)) => unsafe {
                    let _ = DestroyWindow(window);
                },
                Some((true, false)) => copy_and_confirm(window),
                _ => {}
            }
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
            unsafe {
                let _ = DestroyWindow(window);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER => {
            unsafe {
                let _ = KillTimer(Some(window), TIMER);
            }
            repaint(window, false);
            LRESULT(0)
        }
        WM_SETTINGCHANGE | WM_THEMECHANGED => {
            repaint_theme(window, crate::app::light_taskbar());
            LRESULT(0)
        }
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(window);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            DONATION.set(None);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn copy_and_confirm(window: HWND) {
    match copy_address() {
        Ok(()) => {
            repaint(window, true);
            unsafe {
                SetTimer(Some(window), TIMER, 1800, None);
            }
        }
        Err(error) => log!("could not copy the donation address: {error}"),
    }
}

fn repaint(window: HWND, copied: bool) {
    let result = DONATION.with_borrow_mut(|donation| -> Result<Option<((i32, i32), Vec<u8>)>> {
        let Some(donation) = donation.as_mut() else { return Ok(None) };
        donation.copied = copied;
        let strings = content(donation.language, donation.light, copied);
        let (pixels, hits) = donation.card.paint_donation(&strings, &donation.qr, donation.qr_size)?;
        donation.hits = hits;
        Ok(Some((donation.card.size, pixels)))
    });
    match result {
        Ok(Some((size, pixels))) => card::present(window, size, &pixels, None),
        Ok(None) => {}
        Err(error) => log!("could not repaint the donation window: {error}"),
    }
}

fn repaint_theme(window: HWND, light: bool) {
    DONATION.with_borrow_mut(|donation| {
        if let Some(donation) = donation.as_mut() {
            donation.light = light;
        }
    });
    repaint(window, false);
}

fn copy_address() -> Result<()> {
    if !valid_evm_address(ADDRESS) {
        return Err(windows::core::Error::new(windows::core::HRESULT(0x80070057_u32 as i32), "Invalid EVM receive address"));
    }
    let text = clipboard_text();
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, text.len() * size_of::<u16>())?;
        let destination = GlobalLock(memory);
        if destination.is_null() {
            let _ = GlobalFree(Some(memory));
            return Err(windows::core::Error::from_thread());
        }
        copy_nonoverlapping(text.as_ptr(), destination.cast::<u16>(), text.len());
        SetLastError(WIN32_ERROR(0));
        if let Err(error) = GlobalUnlock(memory)
            && GetLastError() != ERROR_SUCCESS
        {
            let _ = GlobalFree(Some(memory));
            return Err(error);
        }
        if let Err(error) = OpenClipboard(None) {
            let _ = GlobalFree(Some(memory));
            return Err(error);
        }
        let _clipboard = ClipboardGuard;
        if let Err(error) = EmptyClipboard() {
            let _ = GlobalFree(Some(memory));
            return Err(error);
        }
        match SetClipboardData(13, Some(HANDLE(memory.0))) {
            Ok(_) => Ok(()),
            Err(error) => {
                let _ = GlobalFree(Some(memory));
                Err(error)
            }
        }
    }
}

struct ClipboardGuard;

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn valid_evm_address(address: &str) -> bool {
    let Some(body) = address.strip_prefix("0x") else { return false };
    if body.len() != 40 || !body.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return false;
    }
    let has_lower = body.bytes().any(|byte| byte.is_ascii_lowercase());
    let has_upper = body.bytes().any(|byte| byte.is_ascii_uppercase());
    if !has_lower || !has_upper {
        return true;
    }

    let lower = body.to_ascii_lowercase();
    let mut hash = [0u8; 32];
    let mut keccak = Keccak::v256();
    keccak.update(lower.as_bytes());
    keccak.finalize(&mut hash);
    body.bytes().enumerate().all(|(index, byte)| {
        if !byte.is_ascii_alphabetic() {
            return true;
        }
        let nibble = if index % 2 == 0 { hash[index / 2] >> 4 } else { hash[index / 2] & 0x0f };
        if nibble >= 8 { byte.is_ascii_uppercase() } else { byte.is_ascii_lowercase() }
    })
}

fn clipboard_text() -> Vec<u16> {
    ADDRESS.encode_utf16().chain([0]).collect()
}

#[cfg(test)]
mod tests {
    use super::{ADDRESS, clipboard_text, valid_evm_address};
    use qrcode::QrCode;

    #[test]
    fn donation_address_has_a_valid_eip55_checksum() {
        assert!(valid_evm_address(ADDRESS));
    }

    #[test]
    fn invalid_or_malformed_addresses_are_rejected() {
        assert!(!valid_evm_address("0xnot-an-address"));
        assert!(!valid_evm_address("0xCc170d39cEE9786CdBe9E75a5586a3294191b91C"));
        assert!(!valid_evm_address("0x00000000000000000000000000000000000000000"));
    }

    #[test]
    fn clipboard_payload_is_the_full_unmodified_address() {
        let payload = clipboard_text();
        assert_eq!(String::from_utf16(&payload[..payload.len() - 1]).unwrap(), ADDRESS);
        assert_eq!(payload.last(), Some(&0));
    }

    #[test]
    fn qr_generation_uses_the_exact_checked_address_as_payload() {
        assert!(valid_evm_address(ADDRESS));
        let code = QrCode::new(ADDRESS.as_bytes()).unwrap();
        assert!(code.width() >= 21);
    }
}
