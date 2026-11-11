use std::cell::RefCell;
use std::mem::size_of;
use std::sync::Once;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::UpdateWindow;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{ICC_BAR_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, TBM_SETPOS, TBM_SETRANGE, TBS_AUTOTICKS};
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, Result, w};

use crate::locale::Language;
use crate::log;
use snowhush_core::layer::{SnowDriftMode, SnowDriftSettings};

const CLASS: PCWSTR = w!("WinterVibeDriftSettings");
const WIDTH: i32 = 560;
const HEIGHT: i32 = 535;
const MODE_ID: u32 = 500;
const SLIDER_IDS: [u32; 5] = [501, 502, 503, 504, 505];
const LABEL_IDS: [u32; 5] = [511, 512, 513, 514, 515];
const SLIDER_RANGES: [(u32, u32); 5] = [(50, 150), (50, 200), (0, 100), (50, 150), (10, 80)];
const SLIDER_Y: [i32; 5] = [132, 203, 274, 345, 416];

type Handler = Box<dyn Fn(SnowDriftSettings)>;

thread_local! {
    static WINDOW: RefCell<Option<DriftSettingsWindow>> = const { RefCell::new(None) };
}

struct DriftSettingsWindow {
    hwnd: HWND,
    mode: HWND,
    sliders: [HWND; 5],
    labels: [HWND; 5],
    language: Language,
    on_change: Handler,
}

pub fn show(owner: HWND, settings: SnowDriftSettings, language: Language, on_change: Handler) {
    if let Some(hwnd) = WINDOW.with_borrow(|window| window.as_ref().map(|window| window.hwnd)) {
        WINDOW.with_borrow_mut(|window| {
            if let Some(window) = window.as_mut() {
                window.on_change = on_change;
                set_controls(window, settings.validated());
                update_labels(window, settings.validated());
            }
        });
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
            let _ = SetForegroundWindow(hwnd);
        }
        return;
    }
    if let Err(error) = open(owner, settings.validated(), language, on_change) {
        log!("snowdrift settings window failed: {error}");
    }
}

pub fn set_enabled(enabled: bool) {
    WINDOW.with_borrow(|window| {
        let Some(window) = window else { return };
        unsafe {
            let _ = EnableWindow(window.mode, enabled);
            for slider in window.sliders {
                let _ = EnableWindow(slider, enabled);
            }
        }
    });
}

fn open(owner: HWND, settings: SnowDriftSettings, language: Language, on_change: Handler) -> Result<()> {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| unsafe {
        let controls = INITCOMMONCONTROLSEX { dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_BAR_CLASSES };
        let _ = InitCommonControlsEx(&controls);
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        let _ = RegisterClassExW(&class);
    });

    let title = language.strings().drift_settings;
    let title_wide = wide(title);
    let x = (unsafe { GetSystemMetrics(SM_CXSCREEN) } - WIDTH) / 2;
    let y = (unsafe { GetSystemMetrics(SM_CYSCREEN) } - HEIGHT) / 2;
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS,
            PCWSTR(title_wide.as_ptr()),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            x,
            y,
            WIDTH,
            HEIGHT,
            Some(owner),
            None,
            Some(GetModuleHandleW(None)?.into()),
            None,
        )?
    };

    let strings = language.strings();
    let heading = unsafe { create_control(window, w!("STATIC"), strings.drift_settings, WS_CHILD | WS_VISIBLE, 24, 18, WIDTH - 54, 30, 0)? };
    let mode_label = unsafe { create_control(window, w!("STATIC"), strings.drift_mode, WS_CHILD | WS_VISIBLE, 24, 61, 190, 25, 0)? };
    let mode = unsafe {
        create_control(
            window,
            w!("COMBOBOX"),
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(CBS_DROPDOWNLIST as u32 | WS_VSCROLL.0),
            215,
            56,
            300,
            180,
            MODE_ID,
        )?
    };
    let _ = (heading, mode_label);

    for (index, label) in mode_titles(language).iter().enumerate() {
        let text = wide(label);
        unsafe {
            let _ = SendMessageW(mode, CB_ADDSTRING, Some(WPARAM(0)), Some(LPARAM(text.as_ptr() as isize)));
        }
        if index == mode_index(settings.mode) {
            unsafe {
                let _ = SendMessageW(mode, CB_SETCURSEL, Some(WPARAM(index)), None);
            }
        }
    }

    let mut sliders = [HWND::default(); 5];
    let mut labels = [HWND::default(); 5];
    for index in 0..5 {
        labels[index] =
            unsafe { create_control(window, w!("STATIC"), "", WS_CHILD | WS_VISIBLE, 24, SLIDER_Y[index] - 25, WIDTH - 54, 24, LABEL_IDS[index])? };
        sliders[index] = unsafe {
            create_control(
                window,
                w!("msctls_trackbar32"),
                "",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(TBS_AUTOTICKS),
                24,
                SLIDER_Y[index],
                WIDTH - 54,
                36,
                SLIDER_IDS[index],
            )?
        };
        let (minimum, maximum) = SLIDER_RANGES[index];
        let range = ((maximum as u16 as u32) << 16 | minimum as u16 as u32) as isize;
        unsafe {
            let _ = SendMessageW(sliders[index], TBM_SETRANGE, Some(WPARAM(1)), Some(LPARAM(range)));
        }
    }

    let state = DriftSettingsWindow { hwnd: window, mode, sliders, labels, language, on_change };
    set_controls(&state, settings);
    update_labels(&state, settings);
    WINDOW.set(Some(state));
    unsafe {
        let _ = ShowWindow(window, SW_SHOWNORMAL);
        let _ = UpdateWindow(window);
    }
    Ok(())
}

unsafe fn create_control(
    parent: HWND,
    class: PCWSTR,
    title: &str,
    style: WINDOW_STYLE,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    id: u32,
) -> Result<HWND> {
    let title = wide(title);
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR(title.as_ptr()),
            style,
            x,
            y,
            width,
            height,
            Some(parent),
            (id != 0).then_some(HMENU(id as _)),
            None,
            None,
        )
    }
}

fn set_controls(window: &DriftSettingsWindow, settings: SnowDriftSettings) {
    unsafe {
        let _ = SendMessageW(window.mode, CB_SETCURSEL, Some(WPARAM(mode_index(settings.mode))), None);
        for (index, value) in
            [settings.height, settings.size, settings.unevenness, settings.accumulation, settings.max_height].into_iter().enumerate()
        {
            let _ = SendMessageW(window.sliders[index], TBM_SETPOS, Some(WPARAM(1)), Some(LPARAM(value as isize)));
        }
    }
}

fn update_labels(window: &DriftSettingsWindow, settings: SnowDriftSettings) {
    let strings = window.language.strings();
    let names = [strings.drift_height, strings.drift_size, strings.drift_unevenness, strings.drift_accumulation, strings.drift_max_height];
    let values = [settings.height, settings.size, settings.unevenness, settings.accumulation, settings.max_height];
    for index in 0..5 {
        let unit = if index == 4 { " px" } else { "%" };
        let value = wide(&format!("{}: {}{}", names[index], values[index], unit));
        if let Err(error) = unsafe { SetWindowTextW(window.labels[index], PCWSTR(value.as_ptr())) } {
            log!("could not update snowdrift slider label: {error}");
        }
    }
}

fn settings_from_controls(window: &DriftSettingsWindow) -> SnowDriftSettings {
    let mode = unsafe { SendMessageW(window.mode, CB_GETCURSEL, None, None).0 };
    let value = |index: usize| unsafe { SendMessageW(window.sliders[index], WM_USER, None, None).0.max(0) as u32 };
    SnowDriftSettings {
        mode: match mode {
            0 => SnowDriftMode::Small,
            1 => SnowDriftMode::Large,
            _ => SnowDriftMode::Mixed,
        },
        height: value(0),
        size: value(1),
        unevenness: value(2),
        accumulation: value(3),
        max_height: value(4),
    }
    .validated()
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_HSCROLL => {
            WINDOW.with_borrow(|state| {
                if let Some(state) = state.as_ref() {
                    let settings = settings_from_controls(state);
                    update_labels(state, settings);
                    (state.on_change)(settings);
                }
            });
            LRESULT(0)
        }
        WM_COMMAND if (wparam.0 >> 16) as u32 == CBN_SELCHANGE => {
            WINDOW.with_borrow(|state| {
                if let Some(state) = state.as_ref()
                    && lparam.0 == state.mode.0 as isize
                {
                    let settings = settings_from_controls(state);
                    update_labels(state, settings);
                    (state.on_change)(settings);
                }
            });
            LRESULT(0)
        }
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(window);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            WINDOW.set(None);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn mode_index(mode: SnowDriftMode) -> usize {
    match mode {
        SnowDriftMode::Small => 0,
        SnowDriftMode::Large => 1,
        SnowDriftMode::Mixed => 2,
    }
}

fn mode_titles(language: Language) -> [&'static str; 3] {
    let strings = language.strings();
    [strings.small_drifts, strings.large_drifts, strings.mixed_drifts]
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
