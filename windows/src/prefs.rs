use std::ffi::c_void;

use windows::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_DWORD, REG_SZ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::Win32::UI::WindowsAndMessaging::{SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW};
use windows::core::{BOOL, PCWSTR, w};

use crate::locale::Language;
use snowhush_core::layer::{SnowDriftMode, SnowDriftSettings};
use snowhush_core::simulation::{SnowPhysics, Snowfall};

const KEY: PCWSTR = w!("Software\\WinterVibe");
const RUN: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("WinterVibe");

pub const SNOW_DENSITIES: [u32; 5] = [100, 200, 300, 400, 500];
pub const CURSOR_FORCE_LEVELS: [u32; 5] = [0, 25, 50, 75, 100];
pub const CURSOR_RADII: [u32; 5] = [60, 90, 120, 160, 200];
pub const INERTIA_LEVELS: [u32; 5] = [0, 35, 65, 85, 100];
pub const VORTEX_LEVELS: [u32; 5] = [0, 15, 30, 50, 75];
pub const WIND_LEVELS: [u32; 5] = [0, 10, 20, 40, 60];
pub const TURBULENCE_LEVELS: [u32; 5] = [0, 15, 25, 50, 75];
pub const FALL_SPEED_LEVELS: [u32; 5] = [50, 75, 100, 125, 150];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum SnowProfile {
    Light = 1,
    #[default]
    Snowfall = 2,
    WeakBlizzard = 3,
    StrongBlizzard = 4,
    Individual = 5,
    Blizzard = 6,
}

impl SnowProfile {
    pub const PRESETS: [SnowProfile; 5] =
        [SnowProfile::Light, SnowProfile::Snowfall, SnowProfile::WeakBlizzard, SnowProfile::StrongBlizzard, SnowProfile::Blizzard];

    fn from_registry(value: u32) -> Option<SnowProfile> {
        Some(match value {
            0 | 1 => SnowProfile::Light,
            2 => SnowProfile::Snowfall,
            3 => SnowProfile::WeakBlizzard,
            4 => SnowProfile::StrongBlizzard,
            5 => SnowProfile::Individual,
            6 => SnowProfile::Blizzard,
            _ => return None,
        })
    }

    fn preset(self) -> Option<ManualSnowSettings> {
        let (snowfall, density, wind, turbulence, fall_speed, cursor_force, cursor_radius, inertia, vortices, height, size, unevenness, accumulation) =
            match self {
                SnowProfile::Light => (Snowfall::Light, 200, 10, 10, 85, 35, 120, 75, 10, 85, 90, 35, 85),
                SnowProfile::Snowfall => (Snowfall::Steady, 150, 20, 20, 100, 50, 120, 65, 20, 100, 100, 50, 100),
                SnowProfile::WeakBlizzard => (Snowfall::Steady, 180, 55, 40, 115, 65, 140, 55, 45, 120, 125, 65, 115),
                SnowProfile::StrongBlizzard => (Snowfall::Heavy, 100, 90, 75, 130, 75, 160, 45, 75, 135, 155, 80, 130),
                SnowProfile::Blizzard => (Snowfall::Heavy, 300, 180, 95, 115, 75, 180, 40, 100, 135, 155, 85, 125),
                SnowProfile::Individual => return None,
            };
        Some(ManualSnowSettings {
            snowfall,
            density,
            physics: SnowPhysics { cursor_enabled: true, cursor_force, cursor_radius, inertia, vortices, wind, turbulence, fall_speed },
            drift: SnowDriftSettings { mode: SnowDriftMode::Mixed, height, size, unevenness, accumulation, max_height: 80 },
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManualSnowSettings {
    pub snowfall: Snowfall,
    pub density: u32,
    pub physics: SnowPhysics,
    pub drift: SnowDriftSettings,
}

#[derive(Clone, Copy)]
pub struct Settings {
    pub language: Language,
    pub snowfall: Snowfall,
    pub density: u32,
    pub physics: SnowPhysics,
    pub drift: SnowDriftSettings,
    pub profile: SnowProfile,
    pub last_profile: SnowProfile,
    manual_snow: ManualSnowSettings,
    pub accumulate: bool,
    pub melt: bool,
    pub snow_on_windows: bool,
    pub behind_windows: bool,
    /// The snow settles at the bottom of the screen, under the taskbar, for a taskbar made see-through, as by TranslucentTB.
    pub behind_taskbar: bool,
    pub show_in_screenshots: bool,
    pub welcomed: bool,
    /// Whether the snow turns spooky on Halloween.
    pub halloween: bool,
    /// Whether leaves fall instead of snow.
    pub autumn: bool,
}

impl Settings {
    pub fn load() -> Settings {
        let calm = !animations_enabled();
        let manual_snow = ManualSnowSettings {
            snowfall: read(w!("snowfall")).and_then(|i| Snowfall::ALL.get(i as usize).copied()).unwrap_or(if calm {
                Snowfall::Light
            } else {
                Snowfall::Steady
            }),
            density: read(w!("snowDensity")).filter(|value| SNOW_DENSITIES.contains(value)).unwrap_or(100),
            physics: SnowPhysics {
                cursor_enabled: read(w!("physicsCursor")).is_none_or(|value| value != 0),
                cursor_force: read(w!("physicsForce")).filter(|value| CURSOR_FORCE_LEVELS.contains(value)).unwrap_or(50),
                cursor_radius: read(w!("physicsRadius")).filter(|value| CURSOR_RADII.contains(value)).unwrap_or(120),
                inertia: read(w!("physicsInertia")).filter(|value| INERTIA_LEVELS.contains(value)).unwrap_or(65),
                vortices: read(w!("physicsVortices")).filter(|value| VORTEX_LEVELS.contains(value)).unwrap_or(30),
                wind: read(w!("physicsWind")).filter(|value| *value <= 180).unwrap_or(20),
                turbulence: read(w!("physicsTurbulence")).filter(|value| TURBULENCE_LEVELS.contains(value)).unwrap_or(25),
                fall_speed: read(w!("physicsFallSpeed")).filter(|value| FALL_SPEED_LEVELS.contains(value)).unwrap_or(100),
            },
            drift: SnowDriftSettings {
                mode: match read(w!("driftMode")) {
                    Some(0) => SnowDriftMode::Small,
                    Some(1) => SnowDriftMode::Large,
                    _ => SnowDriftMode::Mixed,
                },
                height: read(w!("driftHeight")).filter(|value| (50..=150).contains(value)).unwrap_or(100),
                size: read(w!("driftSize")).filter(|value| (50..=200).contains(value)).unwrap_or(100),
                unevenness: read(w!("driftUnevenness")).filter(|value| (0..=100).contains(value)).unwrap_or(50),
                accumulation: read(w!("driftAccumulation")).filter(|value| (50..=150).contains(value)).unwrap_or(100),
                max_height: read(w!("driftMaxHeight")).filter(|value| (10..=80).contains(value)).unwrap_or(80),
            },
        };
        let profile = read(w!("snowProfile")).and_then(SnowProfile::from_registry).unwrap_or(SnowProfile::Individual);
        let last_profile = read(w!("lastSnowProfile"))
            .and_then(SnowProfile::from_registry)
            .filter(|profile| *profile != SnowProfile::Individual)
            .unwrap_or(SnowProfile::Snowfall);
        let mut settings = Settings {
            language: Language::from_registry(read(w!("language"))),
            snowfall: manual_snow.snowfall,
            density: manual_snow.density,
            physics: manual_snow.physics,
            drift: manual_snow.drift,
            profile,
            last_profile,
            manual_snow,
            accumulate: read(w!("accumulate")).is_none_or(|v| v != 0),
            melt: read(w!("meltSnow")).is_none_or(|v| v != 0),
            snow_on_windows: read(w!("snowOnWindows")).is_none_or(|v| v != 0),
            behind_windows: read(w!("behindWindows")).is_some_and(|v| v != 0),
            behind_taskbar: read(w!("behindTaskbar")).is_some_and(|v| v != 0),
            show_in_screenshots: read(w!("showInScreenshots")).is_some_and(|v| v != 0),
            welcomed: read(w!("welcomed")).is_some_and(|v| v != 0),
            halloween: read(w!("halloween")).is_none_or(|v| v != 0),
            autumn: read(w!("autumn")).is_some_and(|v| v != 0),
        };
        if profile != SnowProfile::Individual {
            settings.apply_profile(profile);
        }
        settings
    }

    pub fn select_profile(&mut self, selected: SnowProfile) -> bool {
        if selected == SnowProfile::Individual {
            if self.profile == SnowProfile::Individual {
                self.profile = self.last_profile;
                self.apply_profile(self.profile);
            } else {
                self.profile = SnowProfile::Individual;
                self.apply_manual_settings();
            }
        } else {
            if self.profile == selected {
                return false;
            }
            self.last_profile = selected;
            self.profile = selected;
            self.apply_profile(selected);
        }
        true
    }

    fn capture_manual_settings(&mut self) {
        self.manual_snow = ManualSnowSettings { snowfall: self.snowfall, density: self.density, physics: self.physics, drift: self.drift };
    }

    fn apply_profile(&mut self, profile: SnowProfile) {
        let Some(mut preset) = profile.preset() else { return };
        preset.drift.max_height = self.manual_snow.drift.max_height;
        self.snowfall = preset.snowfall;
        self.density = preset.density;
        self.physics = preset.physics;
        self.drift = preset.drift;
    }

    fn apply_manual_settings(&mut self) {
        self.snowfall = self.manual_snow.snowfall;
        self.density = self.manual_snow.density;
        self.physics = self.manual_snow.physics;
        self.drift = self.manual_snow.drift;
    }

    pub fn save(&mut self) {
        if self.profile == SnowProfile::Individual {
            self.capture_manual_settings();
        }
        let manual_snow = self.manual_snow;
        save_language(self.language);
        write(w!("snowProfile"), self.profile as u32);
        write(w!("lastSnowProfile"), self.last_profile as u32);
        let snowfall = Snowfall::ALL.iter().position(|&s| s == manual_snow.snowfall).unwrap_or(2);
        write(w!("snowfall"), snowfall as u32);
        write(w!("snowDensity"), manual_snow.density);
        write(w!("physicsCursor"), manual_snow.physics.cursor_enabled as u32);
        write(w!("physicsForce"), manual_snow.physics.cursor_force);
        write(w!("physicsRadius"), manual_snow.physics.cursor_radius);
        write(w!("physicsInertia"), manual_snow.physics.inertia);
        write(w!("physicsVortices"), manual_snow.physics.vortices);
        write(w!("physicsWind"), manual_snow.physics.wind);
        write(w!("physicsTurbulence"), manual_snow.physics.turbulence);
        write(w!("physicsFallSpeed"), manual_snow.physics.fall_speed);
        save_drift(manual_snow.drift);
        write(w!("accumulate"), self.accumulate as u32);
        write(w!("meltSnow"), self.melt as u32);
        write(w!("snowOnWindows"), self.snow_on_windows as u32);
        write(w!("behindWindows"), self.behind_windows as u32);
        write(w!("behindTaskbar"), self.behind_taskbar as u32);
        write(w!("showInScreenshots"), self.show_in_screenshots as u32);
        write(w!("welcomed"), self.welcomed as u32);
        write(w!("halloween"), self.halloween as u32);
        write(w!("autumn"), self.autumn as u32);
    }
}

pub fn save_drift(drift: SnowDriftSettings) {
    write(
        w!("driftMode"),
        match drift.mode {
            SnowDriftMode::Small => 0,
            SnowDriftMode::Large => 1,
            SnowDriftMode::Mixed => 2,
        },
    );
    write(w!("driftHeight"), drift.height);
    write(w!("driftSize"), drift.size);
    write(w!("driftUnevenness"), drift.unevenness);
    write(w!("driftAccumulation"), drift.accumulation);
    write(w!("driftMaxHeight"), drift.max_height);
}

pub fn save_language(language: Language) {
    write(w!("language"), language.registry_value());
}

pub fn opens_at_login() -> bool {
    let mut buffer = [0u16; 1024];
    let mut size = (buffer.len() * 2) as u32;
    let status =
        unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN, RUN_VALUE, RRF_RT_REG_SZ, None, Some(buffer.as_mut_ptr().cast::<c_void>()), Some(&mut size)) };
    if status != ERROR_SUCCESS {
        return false;
    }
    let length = buffer.iter().position(|&c| c == 0).unwrap_or(0);
    command().is_some_and(|command| command[..command.len() - 1] == buffer[..length])
}

pub fn set_opens_at_login(on: bool) -> WIN32_ERROR {
    unsafe {
        if !on {
            return RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, RUN_VALUE);
        }
        let Some(command) = command() else { return WIN32_ERROR(1) };
        RegSetKeyValueW(HKEY_CURRENT_USER, RUN, RUN_VALUE, REG_SZ.0, Some(command.as_ptr().cast()), (command.len() * 2) as u32)
    }
}

/// The quoted path of this executable, NUL-terminated.
fn command() -> Option<Vec<u16>> {
    let path = std::env::current_exe().ok()?;
    Some(format!("\"{}\"", path.display()).encode_utf16().chain([0]).collect())
}

fn read(name: PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut size = 4u32;
    let status =
        unsafe { RegGetValueW(HKEY_CURRENT_USER, KEY, name, RRF_RT_REG_DWORD, None, Some((&raw mut value).cast::<c_void>()), Some(&mut size)) };
    (status == ERROR_SUCCESS).then_some(value)
}

fn write(name: PCWSTR, value: u32) {
    unsafe {
        let _ = RegSetKeyValueW(HKEY_CURRENT_USER, KEY, name, REG_DWORD.0, Some((&raw const value).cast::<c_void>()), 4);
    }
}

/// The "Animation effects" setting: off means the person prefers less motion.
pub fn animations_enabled() -> bool {
    let mut enabled = BOOL(1);
    let result = unsafe {
        SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some((&raw mut enabled).cast::<c_void>()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0))
    };
    result.is_err() || enabled.as_bool()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locale::Language;

    fn settings() -> Settings {
        let manual_snow = ManualSnowSettings {
            snowfall: Snowfall::Variable,
            density: 300,
            physics: SnowPhysics { wind: 40, cursor_force: 75, ..SnowPhysics::default() },
            drift: SnowDriftSettings { mode: SnowDriftMode::Large, height: 125, size: 150, unevenness: 70, accumulation: 125, max_height: 60 },
        };
        Settings {
            language: Language::Russian,
            snowfall: manual_snow.snowfall,
            density: manual_snow.density,
            physics: manual_snow.physics,
            drift: manual_snow.drift,
            profile: SnowProfile::Individual,
            last_profile: SnowProfile::Snowfall,
            manual_snow,
            accumulate: true,
            melt: true,
            snow_on_windows: true,
            behind_windows: false,
            behind_taskbar: false,
            show_in_screenshots: false,
            welcomed: true,
            halloween: true,
            autumn: false,
        }
    }

    #[test]
    fn missing_profile_keeps_existing_settings_individual() {
        assert_eq!(Option::<u32>::None.and_then(SnowProfile::from_registry).unwrap_or(SnowProfile::Individual), SnowProfile::Individual);
    }

    #[test]
    fn quiet_profile_registry_values_migrate_to_light_snowfall() {
        assert_eq!(SnowProfile::from_registry(0), Some(SnowProfile::Light));
        assert_eq!(SnowProfile::from_registry(1), Some(SnowProfile::Light));
        assert_eq!(SnowProfile::from_registry(6), Some(SnowProfile::Blizzard));
    }

    #[test]
    fn preset_switching_restores_manual_settings_without_overwriting_them() {
        let mut settings = settings();
        settings.snowfall = Snowfall::Heavy;
        settings.density = 500;
        settings.physics.wind = 60;
        settings.drift.mode = SnowDriftMode::Small;
        settings.capture_manual_settings();
        let manual = settings.manual_snow;
        assert!(settings.select_profile(SnowProfile::StrongBlizzard));
        assert_eq!(settings.profile, SnowProfile::StrongBlizzard);
        assert_ne!(settings.physics, manual.physics);
        assert_eq!(settings.manual_snow, manual);

        assert!(settings.select_profile(SnowProfile::Individual));
        assert_eq!(settings.profile, SnowProfile::Individual);
        assert_eq!(settings.last_profile, SnowProfile::StrongBlizzard);
        assert_eq!(settings.snowfall, manual.snowfall);
        assert_eq!(settings.density, manual.density);
        assert_eq!(settings.physics, manual.physics);
        assert_eq!(settings.drift, manual.drift);
    }

    #[test]
    fn selecting_individual_twice_returns_to_last_preset() {
        let mut settings = settings();
        assert!(settings.select_profile(SnowProfile::WeakBlizzard));
        assert!(settings.select_profile(SnowProfile::Individual));
        assert_eq!(settings.profile, SnowProfile::Individual);
        assert!(settings.select_profile(SnowProfile::Individual));
        assert_eq!(settings.profile, SnowProfile::WeakBlizzard);
        assert_eq!(settings.last_profile, SnowProfile::WeakBlizzard);
    }

    #[test]
    fn presets_keep_natural_drift_shape_and_screen_height_limit() {
        for profile in SnowProfile::PRESETS {
            let settings = profile.preset().expect("preset profile");
            assert_eq!(settings.drift.mode, SnowDriftMode::Mixed);
            assert!(settings.drift.unevenness > 0);
            assert_eq!(settings.drift.max_height, 80);
            assert!((0..=180).contains(&settings.physics.wind));
            assert!((0..=100).contains(&settings.physics.turbulence));
            assert!((50..=150).contains(&settings.physics.fall_speed));
        }
    }

    #[test]
    fn preset_snowfall_targets_increase_from_light_to_blizzard() {
        let rate = |profile: SnowProfile| {
            let preset = profile.preset().expect("preset profile");
            let flakes = match preset.snowfall {
                Snowfall::Off => 0,
                Snowfall::Light => 150,
                Snowfall::Steady | Snowfall::Variable => 380,
                Snowfall::Heavy => 800,
            };
            flakes * preset.density
        };
        let rates: Vec<_> = SnowProfile::PRESETS.into_iter().map(rate).collect();
        assert!(rates.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn blizzard_uses_stronger_horizontal_airflow_than_strong_blizzard() {
        let strong = SnowProfile::StrongBlizzard.preset().expect("strong blizzard profile");
        let blizzard = SnowProfile::Blizzard.preset().expect("blizzard profile");
        assert_eq!(blizzard.density, strong.density * 3);
        assert!(blizzard.physics.wind > strong.physics.wind);
        assert!(blizzard.physics.turbulence > strong.physics.turbulence);
        assert!(blizzard.physics.vortices > strong.physics.vortices);
    }
}
