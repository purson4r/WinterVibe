use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{POWERBROADCAST_SETTING, RegisterPowerSettingNotification};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::System::RemoteDesktop::{NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification};
use windows::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
use windows::Win32::System::Threading::{CreateEventW, SetEvent};
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_SELECT,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, Result, w};

use crate::dialog::{self, Button as DialogButton, Icon as DialogIcon};
use crate::donation;
use crate::drift_settings;
use crate::halloween;
use crate::locale::Language;
use crate::log;
use crate::prefs::{self, Settings, SnowProfile};
use crate::snow::{self, Command, Links};
use crate::update::{self, Release};
use crate::welcome::{self, Handlers};
use snowhush_core::simulation::{SnowPhysics, Snowfall};

const CLASS: PCWSTR = w!("WinterVibeController");
const WM_TRAY: u32 = WM_APP + 1;
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
const ID_SNOWFALL: u32 = 100;
const ID_ACCUMULATE: u32 = 200;
const ID_MELT: u32 = 201;
const ID_SCREENSHOTS: u32 = 202;
const ID_LOGIN: u32 = 203;
const ID_WINDOWS: u32 = 204;
const ID_BEHIND: u32 = 205;
const ID_QUIT: u32 = 299;
const ID_UPDATES: u32 = 206;
const ID_CLEAR: u32 = 207;
const ID_HALLOWEEN: u32 = 208;
const ID_TASKBAR: u32 = 209;
const ID_SNOW: u32 = 210;
const ID_AUTUMN: u32 = 211;
const ID_LANGUAGE_RUSSIAN: u32 = 213;
const ID_LANGUAGE_ENGLISH: u32 = 214;
const ID_SUPPORT_AUTHOR: u32 = 215;
const ID_DENSITY: u32 = 220;
const ID_DENSITY_LAST: u32 = ID_DENSITY + 4;
const ID_PHYSICS_CURSOR: u32 = 230;
const ID_CURSOR_FORCE: u32 = 231;
const ID_CURSOR_FORCE_LAST: u32 = ID_CURSOR_FORCE + 4;
const ID_CURSOR_RADIUS: u32 = 236;
const ID_CURSOR_RADIUS_LAST: u32 = ID_CURSOR_RADIUS + 4;
const ID_FLAKE_INERTIA: u32 = 241;
const ID_FLAKE_INERTIA_LAST: u32 = ID_FLAKE_INERTIA + 4;
const ID_VORTICES: u32 = 246;
const ID_VORTICES_LAST: u32 = ID_VORTICES + 4;
const ID_WIND: u32 = 251;
const ID_WIND_LAST: u32 = ID_WIND + 4;
const ID_TURBULENCE: u32 = 256;
const ID_TURBULENCE_LAST: u32 = ID_TURBULENCE + 4;
const ID_FALL_SPEED: u32 = 261;
const ID_FALL_SPEED_LAST: u32 = ID_FALL_SPEED + 4;
const ID_DRIFT_SETTINGS: u32 = 270;
const ID_PROFILE_LIGHT: u32 = 280;
const ID_PROFILE_LAST: u32 = ID_PROFILE_LIGHT + 4;
const ID_PROFILE_INDIVIDUAL: u32 = ID_PROFILE_LIGHT + 5;
const TIMER_WELCOME: usize = 1;
const TIMER_UPDATES: usize = 2;
const TIMER_HALLOWEEN: usize = 3;
const WM_UPDATE: u32 = WM_APP + 2;
/// The shell's message for a click on the tray notification.
const NIN_BALLOONUSERCLICK: u32 = WM_USER + 5;
const DAY: u32 = 24 * 60 * 60 * 1000;

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// A glyph drawn like the Mac's menu bar ones, at the sizes the tray draws it at.
macro_rules! glyph {
    ($name:literal) => {
        [
            (16, include_bytes!(concat!("../assets/tray/", $name, "-16.png")).as_slice()),
            (20, include_bytes!(concat!("../assets/tray/", $name, "-20.png")).as_slice()),
            (24, include_bytes!(concat!("../assets/tray/", $name, "-24.png")).as_slice()),
            (32, include_bytes!(concat!("../assets/tray/", $name, "-32.png")).as_slice()),
            (40, include_bytes!(concat!("../assets/tray/", $name, "-40.png")).as_slice()),
            (48, include_bytes!(concat!("../assets/tray/", $name, "-48.png")).as_slice()),
            (64, include_bytes!(concat!("../assets/tray/", $name, "-64.png")).as_slice()),
        ]
    };
}

type Glyph = [(i32, &'static [u8]); 7];
const SNOWFLAKE_WHITE: Glyph = glyph!("snowflake-white");
const SNOWFLAKE_BLACK: Glyph = glyph!("snowflake-black");
const LEAF_WHITE: Glyph = glyph!("leaf-white");
const LEAF_BLACK: Glyph = glyph!("leaf-black");
const PUMPKIN_WHITE: Glyph = glyph!("pumpkin-white");
const PUMPKIN_BLACK: Glyph = glyph!("pumpkin-black");

/// What the tray shows: a snowflake, a jack-o'-lantern while it is Halloween and wanted, or a leaf while leaves fall.
#[derive(Clone, Copy, PartialEq)]
enum Emblem {
    Snowflake,
    Pumpkin,
    Leaf,
}

impl Emblem {
    fn of(settings: &Settings) -> Emblem {
        if settings.halloween && halloween::season() {
            Emblem::Pumpkin
        } else if settings.autumn {
            Emblem::Leaf
        } else {
            Emblem::Snowflake
        }
    }

    /// Black on a light taskbar, white on a dark one, as the system's own tray icons.
    fn glyph(self, light: bool) -> &'static Glyph {
        match (self, light) {
            (Emblem::Snowflake, false) => &SNOWFLAKE_WHITE,
            (Emblem::Snowflake, true) => &SNOWFLAKE_BLACK,
            (Emblem::Leaf, false) => &LEAF_WHITE,
            (Emblem::Leaf, true) => &LEAF_BLACK,
            (Emblem::Pumpkin, false) => &PUMPKIN_WHITE,
            (Emblem::Pumpkin, true) => &PUMPKIN_BLACK,
        }
    }
}

/// Whether the taskbar is light: the setting for "your default Windows mode".
pub fn light_taskbar() -> bool {
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast::<c_void>()),
            Some(&mut size),
        )
    };
    status.is_ok() && value == 1
}

struct App {
    settings: Settings,
    commands: Sender<Command>,
    wake: HANDLE,
    snow: Option<JoinHandle<()>>,
    icon: HICON,
    emblem: Emblem,
    light: bool,
    locked: bool,
    display_off: bool,
    away: bool,
    /// The download of the update the tray last announced.
    update: Option<String>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// The tray icon and its menu, and the system events that pause the snow, on the main thread;
/// the snow itself runs on its own thread so menus and other modal loops never stall it.
pub fn run() -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&class);
        let window = CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, w!("WinterVibe"), WS_OVERLAPPED, 0, 0, 0, 0, None, None, Some(instance.into()), None)?;
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);

        let settings = Settings::load();
        let (commands, receiver) = mpsc::channel();
        let wake = CreateEventW(None, false, false, None)?;
        let links = Links { wake: wake.0 as isize, controller: window.0 as isize };
        let snow = thread::Builder::new().name("snow".into()).spawn(move || snow::run(receiver, links, settings)).ok();
        let emblem = Emblem::of(&settings);
        let light = light_taskbar();
        let icon = tray_icon(emblem, light);
        add_tray_icon(window, icon, settings.language);
        let _ = WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION);
        let _ = RegisterPowerSettingNotification(HANDLE(window.0), &GUID_CONSOLE_DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE);
        if !settings.welcomed {
            SetTimer(Some(window), TIMER_WELCOME, 1200, None);
        }
        SetTimer(Some(window), TIMER_UPDATES, 30_000, None);
        SetTimer(Some(window), TIMER_HALLOWEEN, 60_000, None);
        APP.set(Some(App { settings, commands, wake, snow, icon, emblem, light, locked: false, display_off: false, away: false, update: None }));

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_TRAY => {
            let event = (lparam.0 & 0xffff) as u32;
            if event == WM_CONTEXTMENU || event == NIN_SELECT || event == NIN_KEYSELECT {
                let x = (wparam.0 & 0xffff) as i16 as i32;
                let y = ((wparam.0 >> 16) & 0xffff) as i16 as i32;
                show_menu(window, x, y);
            } else if event == NIN_BALLOONUSERCLICK
                && let Some(url) = APP.with_borrow(|app| app.as_ref().and_then(|app| app.update.clone()))
            {
                update::open(&url);
            }
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            update(|app| match wparam.0 as u32 {
                WTS_SESSION_LOCK => app.locked = true,
                WTS_SESSION_UNLOCK => app.locked = false,
                WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => app.away = true,
                WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => app.away = false,
                _ => {}
            });
            LRESULT(0)
        }
        WM_POWERBROADCAST if wparam.0 as u32 == PBT_POWERSETTINGCHANGE => {
            let setting = unsafe { &*(lparam.0 as *const POWERBROADCAST_SETTING) };
            if setting.PowerSetting == GUID_CONSOLE_DISPLAY_STATE {
                let off = setting.Data[0] == 0;
                update(|app| app.display_off = off);
            }
            LRESULT(1)
        }
        WM_CLOSE => {
            quit(window);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_UPDATES => {
            unsafe { SetTimer(Some(window), TIMER_UPDATES, DAY, None) };
            check_for_updates(window, false);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_HALLOWEEN => {
            refresh_icon(window);
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            refresh_icon(window);
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        WM_UPDATE => {
            let result = unsafe { Box::from_raw(lparam.0 as *mut std::result::Result<Option<Release>, String>) };
            updated(window, *result, wparam.0 != 0);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_WELCOME => {
            unsafe {
                let _ = KillTimer(Some(window), TIMER_WELCOME);
            }
            let language = APP.with_borrow(|app| app.as_ref().map_or(Language::Russian, |app| app.settings.language));
            welcome::show(
                language,
                Handlers {
                    scatter: Box::new(|pieces| {
                        APP.with_borrow(|app| {
                            if let Some(app) = app {
                                send(app, Command::Scatter(pieces));
                            }
                        })
                    }),
                    finished: Box::new(|| {
                        APP.with_borrow_mut(|app| {
                            let Some(app) = app else { return };
                            app.settings.welcomed = true;
                            app.settings.save();
                        })
                    }),
                    skipped: Box::new(|| {
                        APP.with_borrow(|app| {
                            if let Some(app) = app {
                                send(app, Command::ReleaseSnowfall);
                            }
                        })
                    }),
                },
            );
            LRESULT(0)
        }
        _ if message != 0 && message == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            APP.with_borrow(|app| {
                if let Some(app) = app {
                    add_tray_icon(window, app.icon, app.settings.language);
                    send(app, Command::ShellRestarted);
                }
            });
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

/// Changes what pauses the snow and tells the snow thread whether it may run.
fn update(change: impl FnOnce(&mut App)) {
    APP.with_borrow_mut(|app| {
        let Some(app) = app else { return };
        change(app);
        let active = !app.locked && !app.display_off && !app.away;
        send(app, Command::Active(active));
    });
}

fn send(app: &App, command: Command) {
    let _ = app.commands.send(command);
    unsafe {
        let _ = SetEvent(app.wake);
    }
}

fn append_menu_item(menu: HMENU, flags: MENU_ITEM_FLAGS, id: usize, title: &str) {
    let title: Vec<u16> = title.encode_utf16().chain([0]).collect();
    unsafe {
        let _ = AppendMenuW(menu, flags, id, PCWSTR(title.as_ptr()));
    }
}

fn append_physics_options(menu: HMENU, first_id: u32, current: u32, options: &[u32], unit: &str) {
    for (index, value) in options.iter().enumerate() {
        append_menu_item(menu, MF_STRING, (first_id + index as u32) as usize, &format!("{value}{unit}"));
    }
    if let Some(index) = options.iter().position(|&value| value == current) {
        let selected = first_id + index as u32;
        let last = first_id + options.len() as u32 - 1;
        let _ = unsafe { CheckMenuRadioItem(menu, first_id, last, selected, MF_BYCOMMAND.0) };
    }
}

fn snowfall_title(language: Language, snowfall: Snowfall) -> &'static str {
    let strings = language.strings();
    match snowfall {
        Snowfall::Off => strings.off,
        Snowfall::Light => strings.light,
        Snowfall::Steady => strings.steady,
        Snowfall::Heavy => strings.heavy,
        Snowfall::Variable => strings.variable,
    }
}

fn profile_title(language: Language, profile: SnowProfile) -> &'static str {
    let strings = language.strings();
    match profile {
        SnowProfile::Light => strings.light_snowfall,
        SnowProfile::Snowfall => strings.snowfall_profile,
        SnowProfile::WeakBlizzard => strings.weak_blizzard,
        SnowProfile::StrongBlizzard => strings.strong_blizzard,
        SnowProfile::Blizzard => strings.blizzard,
        SnowProfile::Individual => strings.individual_settings,
    }
}

fn show_menu(window: HWND, x: i32, y: i32) {
    loop {
        let Some(settings) = APP.with_borrow(|app| app.as_ref().map(|app| app.settings)) else { return };
        let chosen = show_menu_once(window, x, y, settings);
        if chosen == 0 {
            return;
        }
        choose(window, chosen, prefs::opens_at_login());
        if matches!(chosen, ID_QUIT | ID_SUPPORT_AUTHOR | ID_DRIFT_SETTINGS) {
            return;
        }
    }
}

fn show_menu_once(window: HWND, x: i32, y: i32, settings: Settings) -> u32 {
    let strings = settings.language.strings();
    let login = prefs::opens_at_login();
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return 0 };
        let Ok(snowfall) = CreatePopupMenu() else { return 0 };
        let Ok(modes) = CreatePopupMenu() else { return 0 };
        let Ok(languages) = CreatePopupMenu() else { return 0 };
        let Ok(densities) = CreatePopupMenu() else { return 0 };
        let Ok(physics_menu) = CreatePopupMenu() else { return 0 };
        let Ok(cursor_force) = CreatePopupMenu() else { return 0 };
        let Ok(cursor_radius) = CreatePopupMenu() else { return 0 };
        let Ok(inertia) = CreatePopupMenu() else { return 0 };
        let Ok(vortices) = CreatePopupMenu() else { return 0 };
        let Ok(wind) = CreatePopupMenu() else { return 0 };
        let Ok(turbulence) = CreatePopupMenu() else { return 0 };
        let Ok(fall_speed) = CreatePopupMenu() else { return 0 };
        append_menu_item(menu, MF_STRING | MF_GRAYED, 0, strings.snowfall_profiles);
        for (index, profile) in SnowProfile::PRESETS.iter().copied().enumerate() {
            let enabled = if settings.profile == SnowProfile::Individual { MF_GRAYED } else { MF_ENABLED };
            append_menu_item(menu, MF_STRING | enabled, (ID_PROFILE_LIGHT + index as u32) as usize, profile_title(settings.language, profile));
        }
        if settings.profile != SnowProfile::Individual {
            let _ = CheckMenuRadioItem(menu, ID_PROFILE_LIGHT, ID_PROFILE_LAST, profile_menu_id(settings.profile), MF_BYCOMMAND.0);
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_menu_item(
            menu,
            MF_STRING | if settings.profile == SnowProfile::Individual { MF_CHECKED } else { MF_UNCHECKED },
            ID_PROFILE_INDIVIDUAL as usize,
            strings.individual_settings,
        );
        let _ = SetMenuDefaultItem(menu, ID_PROFILE_INDIVIDUAL, 0);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let manual_options = if settings.profile == SnowProfile::Individual { MF_ENABLED } else { MF_GRAYED };
        for (i, option) in Snowfall::ALL.iter().enumerate() {
            append_menu_item(snowfall, MF_STRING, (ID_SNOWFALL + i as u32) as usize, snowfall_title(settings.language, *option));
        }
        let current = Snowfall::ALL.iter().position(|&s| s == settings.snowfall).unwrap_or(0) as u32;
        let _ = CheckMenuRadioItem(snowfall, ID_SNOWFALL, ID_SNOWFALL + 4, ID_SNOWFALL + current, MF_BYCOMMAND.0);
        for (i, density) in prefs::SNOW_DENSITIES.iter().enumerate() {
            append_menu_item(densities, MF_STRING, (ID_DENSITY + i as u32) as usize, &format!("{density}%"));
        }
        let selected_density = prefs::SNOW_DENSITIES.iter().position(|&density| density == settings.density).unwrap_or(0) as u32;
        let _ = CheckMenuRadioItem(densities, ID_DENSITY, ID_DENSITY + 4, ID_DENSITY + selected_density, MF_BYCOMMAND.0);
        append_menu_item(
            physics_menu,
            MF_STRING | if settings.physics.cursor_enabled { MF_CHECKED } else { MF_UNCHECKED },
            ID_PHYSICS_CURSOR as usize,
            strings.cursor_interaction,
        );
        append_physics_options(cursor_force, ID_CURSOR_FORCE, settings.physics.cursor_force, &prefs::CURSOR_FORCE_LEVELS, "%");
        append_physics_options(cursor_radius, ID_CURSOR_RADIUS, settings.physics.cursor_radius, &prefs::CURSOR_RADII, " px");
        append_physics_options(inertia, ID_FLAKE_INERTIA, settings.physics.inertia, &prefs::INERTIA_LEVELS, "%");
        append_physics_options(vortices, ID_VORTICES, settings.physics.vortices, &prefs::VORTEX_LEVELS, "%");
        append_physics_options(wind, ID_WIND, settings.physics.wind, &prefs::WIND_LEVELS, "%");
        append_physics_options(turbulence, ID_TURBULENCE, settings.physics.turbulence, &prefs::TURBULENCE_LEVELS, "%");
        append_physics_options(fall_speed, ID_FALL_SPEED, settings.physics.fall_speed, &prefs::FALL_SPEED_LEVELS, "%");
        append_menu_item(physics_menu, MF_POPUP, cursor_force.0 as usize, strings.cursor_force);
        append_menu_item(physics_menu, MF_POPUP, cursor_radius.0 as usize, strings.cursor_radius);
        append_menu_item(physics_menu, MF_POPUP, inertia.0 as usize, strings.inertia);
        append_menu_item(physics_menu, MF_POPUP, vortices.0 as usize, strings.vortices);
        append_menu_item(physics_menu, MF_POPUP, wind.0 as usize, strings.wind_strength);
        append_menu_item(physics_menu, MF_POPUP, turbulence.0 as usize, strings.turbulence);
        append_menu_item(physics_menu, MF_POPUP, fall_speed.0 as usize, strings.fall_speed);
        append_menu_item(modes, MF_STRING, ID_SNOW as usize, strings.snow);
        append_menu_item(modes, MF_STRING, ID_AUTUMN as usize, strings.autumn_leaves);
        let _ = CheckMenuRadioItem(modes, ID_SNOW, ID_AUTUMN, if settings.autumn { ID_AUTUMN } else { ID_SNOW }, MF_BYCOMMAND.0);
        let checked = |on: bool| if on { MF_CHECKED } else { MF_UNCHECKED };
        let autumn = settings.autumn;
        append_menu_item(menu, MF_POPUP | manual_options, snowfall.0 as usize, strings.intensity);
        append_menu_item(menu, MF_POPUP | manual_options, densities.0 as usize, strings.density);
        append_menu_item(menu, MF_POPUP | manual_options, modes.0 as usize, strings.mode);
        append_menu_item(menu, MF_POPUP | manual_options, physics_menu.0 as usize, strings.physics);
        append_menu_item(menu, MF_STRING | manual_options, ID_DRIFT_SETTINGS as usize, strings.drift_settings);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_menu_item(menu, MF_STRING | checked(settings.accumulate), ID_ACCUMULATE as usize, strings.accumulate);
        let melt = MF_STRING | checked(settings.melt) | if settings.accumulate { MF_ENABLED } else { MF_GRAYED };
        append_menu_item(menu, melt, ID_MELT as usize, if autumn { strings.wither_leaves } else { strings.melt_snow });
        let windows = MF_STRING | checked(settings.snow_on_windows);
        append_menu_item(menu, windows, ID_WINDOWS as usize, if autumn { strings.leaves_on_windows } else { strings.snow_on_windows });
        append_menu_item(menu, MF_STRING, ID_CLEAR as usize, if autumn { strings.clear_leaves } else { strings.clear_snow });
        if halloween::season() {
            append_menu_item(menu, MF_STRING | checked(settings.halloween), ID_HALLOWEEN as usize, strings.halloween);
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_menu_item(menu, MF_STRING | checked(settings.behind_windows), ID_BEHIND as usize, strings.behind_windows);
        append_menu_item(menu, MF_STRING | checked(settings.behind_taskbar), ID_TASKBAR as usize, strings.behind_taskbar);
        append_menu_item(menu, MF_STRING | checked(settings.show_in_screenshots), ID_SCREENSHOTS as usize, strings.show_in_screenshots);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_menu_item(menu, MF_STRING | checked(login), ID_LOGIN as usize, strings.open_at_login);
        append_menu_item(menu, MF_STRING, ID_UPDATES as usize, strings.check_updates);
        append_menu_item(languages, MF_STRING, ID_LANGUAGE_RUSSIAN as usize, strings.russian);
        append_menu_item(languages, MF_STRING, ID_LANGUAGE_ENGLISH as usize, strings.english);
        let selected_language = if settings.language == Language::Russian { ID_LANGUAGE_RUSSIAN } else { ID_LANGUAGE_ENGLISH };
        let _ = CheckMenuRadioItem(languages, ID_LANGUAGE_RUSSIAN, ID_LANGUAGE_ENGLISH, selected_language, MF_BYCOMMAND.0);
        append_menu_item(menu, MF_POPUP, languages.0 as usize, strings.language_menu);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_menu_item(menu, MF_STRING, ID_SUPPORT_AUTHOR as usize, strings.support_author_menu);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        append_menu_item(menu, MF_STRING, ID_QUIT as usize, strings.quit);

        // Recreate the native menu after a command so its checks and enabled states stay current.
        let _ = SetForegroundWindow(window);
        let chosen = TrackPopupMenuEx(menu, (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY).0, x, y, window, None).0 as u32;
        let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        chosen
    }
}

fn choose(window: HWND, chosen: u32, login: bool) {
    match chosen {
        0 => {}
        ID_QUIT => quit(window),
        ID_UPDATES => check_for_updates(window, true),
        ID_PROFILE_LIGHT..=ID_PROFILE_INDIVIDUAL => {
            let selected = SnowProfile::PRESETS.get((chosen - ID_PROFILE_LIGHT) as usize).copied().unwrap_or(SnowProfile::Individual);
            APP.with_borrow_mut(|app| {
                let Some(app) = app else { return };
                if !app.settings.select_profile(selected) {
                    return;
                }
                app.settings.save();
                let settings = &app.settings;
                drift_settings::set_enabled(settings.profile == SnowProfile::Individual);
                send(app, Command::Snowfall(settings.snowfall));
                send(app, Command::Density(settings.density));
                send(app, Command::Physics(settings.physics));
                send(app, Command::DriftSettings(settings.drift));
            });
        }
        ID_SUPPORT_AUTHOR => {
            let language = APP.with_borrow(|app| app.as_ref().map_or(Language::Russian, |app| app.settings.language));
            donation::show(language, light_taskbar());
        }
        ID_DRIFT_SETTINGS => {
            let Some((settings, language)) = APP.with_borrow(|app| app.as_ref().map(|app| (app.settings.drift, app.settings.language))) else {
                return;
            };
            if APP.with_borrow(|app| app.as_ref().is_none_or(|app| app.settings.profile != SnowProfile::Individual)) {
                return;
            }
            drift_settings::show(
                window,
                settings,
                language,
                Box::new(|drift| {
                    APP.with_borrow_mut(|app| {
                        let Some(app) = app else { return };
                        if app.settings.profile != SnowProfile::Individual {
                            return;
                        }
                        app.settings.drift = drift;
                        app.settings.save();
                        send(app, Command::DriftSettings(drift));
                    });
                }),
            );
        }
        ID_LANGUAGE_RUSSIAN | ID_LANGUAGE_ENGLISH => {
            let language = if chosen == ID_LANGUAGE_RUSSIAN { Language::Russian } else { Language::English };
            APP.with_borrow_mut(|app| {
                let Some(app) = app else { return };
                if app.settings.language == language {
                    return;
                }
                app.settings.language = language;
                prefs::save_language(language);
                update_tray_tooltip(window, language);
                send(app, Command::Language(language));
            });
        }
        ID_CLEAR => APP.with_borrow(|app| {
            if let Some(app) = app {
                send(app, Command::ClearSnow(!prefs::animations_enabled()));
            }
        }),
        ID_LOGIN => {
            let status = prefs::set_opens_at_login(!login);
            log!("open at login {}: {status:?}", !login);
        }
        ID_HALLOWEEN => {
            APP.with_borrow_mut(|app| {
                let Some(app) = app else { return };
                app.settings.halloween = !app.settings.halloween;
                app.settings.save();
                send(app, Command::Halloween(app.settings.halloween));
            });
            refresh_icon(window);
        }
        ID_SNOW | ID_AUTUMN => {
            if APP.with_borrow(|app| app.as_ref().is_none_or(|app| app.settings.profile != SnowProfile::Individual)) {
                return;
            }
            let autumn = chosen == ID_AUTUMN;
            APP.with_borrow_mut(|app| {
                let Some(app) = app else { return };
                if autumn == app.settings.autumn {
                    return;
                }
                app.settings.autumn = autumn;
                app.settings.save();
                send(app, Command::SwitchMode { autumn, calm: !prefs::animations_enabled() });
            });
            refresh_icon(window);
        }
        _ => APP.with_borrow_mut(|app| {
            let Some(app) = app else { return };
            let settings = &mut app.settings;
            if settings.profile != SnowProfile::Individual && is_manual_setting(chosen) {
                return;
            }
            if let Some(physics) = physics_choice(chosen, settings.physics) {
                settings.physics = physics;
                settings.save();
                send(app, Command::Physics(physics));
                return;
            }
            let command = match chosen {
                ID_ACCUMULATE => {
                    settings.accumulate = !settings.accumulate;
                    Command::Accumulate(settings.accumulate)
                }
                ID_MELT => {
                    settings.melt = !settings.melt;
                    Command::Melt(settings.melt)
                }
                ID_WINDOWS => {
                    settings.snow_on_windows = !settings.snow_on_windows;
                    Command::SnowOnWindows(settings.snow_on_windows)
                }
                ID_BEHIND => {
                    settings.behind_windows = !settings.behind_windows;
                    Command::BehindWindows(settings.behind_windows)
                }
                ID_TASKBAR => {
                    settings.behind_taskbar = !settings.behind_taskbar;
                    Command::BehindTaskbar(settings.behind_taskbar)
                }
                ID_SCREENSHOTS => {
                    settings.show_in_screenshots = !settings.show_in_screenshots;
                    Command::ShowInScreenshots(settings.show_in_screenshots)
                }
                ID_DENSITY..=ID_DENSITY_LAST => {
                    let density = prefs::SNOW_DENSITIES[(chosen - ID_DENSITY) as usize];
                    settings.density = density;
                    Command::Density(density)
                }
                _ => {
                    let Some(&snowfall) = Snowfall::ALL.get(chosen.wrapping_sub(ID_SNOWFALL) as usize) else { return };
                    settings.snowfall = snowfall;
                    Command::Snowfall(snowfall)
                }
            };
            settings.save();
            send(app, command);
        }),
    }
}

fn is_manual_setting(chosen: u32) -> bool {
    (ID_SNOWFALL..=ID_SNOWFALL + 4).contains(&chosen)
        || (ID_DENSITY..=ID_DENSITY_LAST).contains(&chosen)
        || chosen == ID_SNOW
        || chosen == ID_AUTUMN
        || chosen == ID_PHYSICS_CURSOR
        || (ID_CURSOR_FORCE..=ID_CURSOR_FORCE_LAST).contains(&chosen)
        || (ID_CURSOR_RADIUS..=ID_CURSOR_RADIUS_LAST).contains(&chosen)
        || (ID_FLAKE_INERTIA..=ID_FLAKE_INERTIA_LAST).contains(&chosen)
        || (ID_VORTICES..=ID_VORTICES_LAST).contains(&chosen)
        || (ID_WIND..=ID_WIND_LAST).contains(&chosen)
        || (ID_TURBULENCE..=ID_TURBULENCE_LAST).contains(&chosen)
        || (ID_FALL_SPEED..=ID_FALL_SPEED_LAST).contains(&chosen)
        || chosen == ID_DRIFT_SETTINGS
}

fn profile_menu_id(profile: SnowProfile) -> u32 {
    SnowProfile::PRESETS.iter().position(|candidate| *candidate == profile).map_or(ID_PROFILE_INDIVIDUAL, |index| ID_PROFILE_LIGHT + index as u32)
}

fn physics_choice(chosen: u32, mut physics: SnowPhysics) -> Option<SnowPhysics> {
    match chosen {
        ID_PHYSICS_CURSOR => physics.cursor_enabled = !physics.cursor_enabled,
        ID_CURSOR_FORCE..=ID_CURSOR_FORCE_LAST => physics.cursor_force = prefs::CURSOR_FORCE_LEVELS[(chosen - ID_CURSOR_FORCE) as usize],
        ID_CURSOR_RADIUS..=ID_CURSOR_RADIUS_LAST => physics.cursor_radius = prefs::CURSOR_RADII[(chosen - ID_CURSOR_RADIUS) as usize],
        ID_FLAKE_INERTIA..=ID_FLAKE_INERTIA_LAST => physics.inertia = prefs::INERTIA_LEVELS[(chosen - ID_FLAKE_INERTIA) as usize],
        ID_VORTICES..=ID_VORTICES_LAST => physics.vortices = prefs::VORTEX_LEVELS[(chosen - ID_VORTICES) as usize],
        ID_WIND..=ID_WIND_LAST => physics.wind = prefs::WIND_LEVELS[(chosen - ID_WIND) as usize],
        ID_TURBULENCE..=ID_TURBULENCE_LAST => physics.turbulence = prefs::TURBULENCE_LEVELS[(chosen - ID_TURBULENCE) as usize],
        ID_FALL_SPEED..=ID_FALL_SPEED_LAST => physics.fall_speed = prefs::FALL_SPEED_LEVELS[(chosen - ID_FALL_SPEED) as usize],
        _ => return None,
    }
    Some(physics)
}

/// Checks on another thread and reports back to the window with `WM_UPDATE`.
fn check_for_updates(window: HWND, asked: bool) {
    let target = window.0 as isize;
    thread::spawn(move || {
        let result = Box::new(update::check());
        unsafe {
            let _ = PostMessageW(Some(HWND(target as _)), WM_UPDATE, WPARAM(asked as usize), LPARAM(Box::into_raw(result) as isize));
        }
    });
}

/// A check someone asked for always answers; the daily one only speaks up, once, about a new version.
fn updated(window: HWND, result: std::result::Result<Option<Release>, String>, asked: bool) {
    let current = env!("CARGO_PKG_VERSION");
    let language = APP.with_borrow(|app| app.as_ref().map_or(Language::Russian, |app| app.settings.language));
    let strings = language.strings();
    match result {
        Ok(Some(release)) if asked => {
            let instruction = strings.update_available.replace("{version}", &release.version).replace("{current}", current);
            let result = dialog::show(
                Some(window),
                "WinterVibe",
                &instruction,
                strings.download_now,
                &[DialogButton { id: IDYES.0, label: strings.yes }, DialogButton { id: IDNO.0, label: strings.no }],
                IDYES.0,
                DialogIcon::Information,
            );
            if result == IDYES.0 {
                update::open(&release.url);
            }
        }
        Ok(Some(release)) => {
            let fresh = APP.with_borrow_mut(|app| {
                let app = app.as_mut()?;
                (app.update.as_deref() != Some(&release.url)).then(|| app.update = Some(release.url.clone()))
            });
            if fresh.is_some() {
                let notification = strings.update_available.replace("{version}", &release.version).replace("{current}", current);
                notify(window, &notification, strings.click_to_download);
            }
        }
        Ok(None) if asked => {
            dialog::show(
                Some(window),
                "WinterVibe",
                strings.up_to_date,
                &strings.latest_version.replace("{version}", current),
                &[DialogButton { id: IDOK.0, label: strings.ok }],
                IDOK.0,
                DialogIcon::Information,
            );
        }
        Ok(None) => {}
        Err(error) => {
            log!("update check failed: {error}");
            if asked {
                dialog::show(
                    Some(window),
                    "WinterVibe",
                    strings.update_failed,
                    strings.update_failed_details,
                    &[DialogButton { id: IDOK.0, label: strings.ok }],
                    IDOK.0,
                    DialogIcon::Warning,
                );
            }
        }
    }
}

/// A notification from the tray icon.
fn notify(window: HWND, title: &str, text: &str) {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: 1,
        uFlags: NIF_INFO,
        dwInfoFlags: NIIF_INFO,
        ..Default::default()
    };
    for (slot, unit) in data.szInfoTitle.iter_mut().zip(title.encode_utf16().take(63)) {
        *slot = unit;
    }
    for (slot, unit) in data.szInfo.iter_mut().zip(text.encode_utf16().take(255)) {
        *slot = unit;
    }
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
    }
}

fn quit(window: HWND) {
    remove_tray_icon(window);
    if let Some(mut app) = APP.with_borrow_mut(Option::take) {
        send(&app, Command::Quit);
        if let Some(snow) = app.snow.take() {
            let _ = snow.join();
        }
        unsafe {
            let _ = DestroyIcon(app.icon);
        }
    }
    unsafe { PostQuitMessage(0) };
}

/// Puts a jack-o'-lantern in the tray while it is Halloween and wanted, a leaf while leaves fall, and the snowflake
/// back after, recoloured whenever the taskbar turns light or dark.
fn refresh_icon(window: HWND) {
    APP.with_borrow_mut(|app| {
        let Some(app) = app else { return };
        let emblem = Emblem::of(&app.settings);
        let light = light_taskbar();
        if emblem == app.emblem && light == app.light {
            return;
        }
        app.emblem = emblem;
        app.light = light;
        let icon = tray_icon(emblem, light);
        let data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: 1,
            uFlags: NIF_ICON,
            hIcon: icon,
            ..Default::default()
        };
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
            let _ = DestroyIcon(app.icon);
        }
        app.icon = icon;
    });
}

/// The tray's icon at its size for the display's scale, or when its picture can't be had, the app icon from the
/// executable's resources.
fn tray_icon(emblem: Emblem, light: bool) -> HICON {
    unsafe {
        let size = GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem());
        if let Some(icon) = picture_icon(emblem.glyph(light), size) {
            return icon;
        }
        let Ok(instance) = GetModuleHandleW(None) else { return HICON::default() };
        LoadImageW(Some(instance.into()), PCWSTR(1 as *const u16), IMAGE_ICON, size, size, LR_DEFAULTCOLOR)
            .map_or(HICON::default(), |icon| HICON(icon.0))
    }
}

/// The picture of `pictures` closest above `size` pixels across, or the largest, as an icon that size.
fn picture_icon(pictures: &[(i32, &[u8])], size: i32) -> Option<HICON> {
    let (_, picture) = pictures.iter().find(|(side, _)| *side >= size).or(pictures.last())?;
    // Icon resources have kept the version Windows 3 gave them; a PNG is taken as one.
    unsafe { CreateIconFromResourceEx(picture, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR) }.ok()
}

fn add_tray_icon(window: HWND, icon: HICON, language: Language) {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
        uCallbackMessage: WM_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    for (slot, unit) in data.szTip.iter_mut().zip(language.strings().tray_tooltip.encode_utf16()) {
        *slot = unit;
    }
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        let _ = Shell_NotifyIconW(NIM_ADD, &data);
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
    }
}

fn update_tray_tooltip(window: HWND, language: Language) {
    let mut data = NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: window, uID: 1, uFlags: NIF_TIP, ..Default::default() };
    for (slot, unit) in data.szTip.iter_mut().zip(language.strings().tray_tooltip.encode_utf16()) {
        *slot = unit;
    }
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
    }
}

fn remove_tray_icon(window: HWND) {
    let data = NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: window, uID: 1, ..Default::default() };
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
    }
}
