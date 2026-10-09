use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const MAX_GRID: u32 = 64;
pub const MAX_SCALE: u32 = 8;
pub const MAX_IMAGE: u32 = 512;
/// Longest preset name we keep: long enough for real names, short enough for a dropdown.
pub const MAX_NAME: usize = 64;

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum Mode {
    #[default]
    Lines,
    Pixels,
    Image,
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum Theme {
    /// Follow Windows' light or dark app mode.
    #[default]
    System,
    Dark,
    Light,
}

/// What happens to the crosshair while you aim down sights.
#[derive(Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum Aim {
    #[default]
    Keep,
    Hide,
    Preset(String),
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum AimButton {
    #[default]
    Right,
    Middle,
    Back,
    Forward,
}

impl AimButton {
    pub const ALL: [AimButton; 4] = [Self::Right, Self::Middle, Self::Back, Self::Forward];

    pub fn vk(self) -> i32 {
        match self {
            Self::Right => 0x02,
            Self::Middle => 0x04,
            Self::Back => 0x05,
            Self::Forward => 0x06,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Right => "Right mouse button",
            Self::Middle => "Middle mouse button",
            Self::Back => "Mouse back button",
            Self::Forward => "Mouse forward button",
        }
    }
}

/// How aiming works in one place: what happens, and which button means "I'm aiming".
#[derive(Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AimRule {
    pub action: Aim,
    pub button: AimButton,
    pub toggle: bool, // press once to aim, again to stop (games with toggle ADS)
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Crosshair {
    pub mode: Mode,
    // Lines mode
    pub color: [u8; 4], // RGBA, unmultiplied
    pub outline_color: [u8; 4],
    pub outline: u32,
    pub length: u32,
    pub thickness: u32,
    pub gap: u32,
    pub t_style: bool,
    pub dot: bool,
    pub dot_size: u32,
    pub circle: bool,
    pub circle_radius: u32,
    pub circle_thickness: u32,
    // Pixels mode: grid x grid cells, row-major, each drawn as scale x scale screen pixels
    pub grid: u32,
    pub scale: u32,
    pub pixels: Vec<[u8; 4]>,
    // Image mode: a picture copied into images_dir(), scaled so its longer side is image_size
    pub image: String,
    pub image_size: u32,
    pub image_opacity: u32, // percent
    pub offset_x: i32,
    pub offset_y: i32,
}

impl Default for Crosshair {
    fn default() -> Self {
        Self {
            mode: Mode::Lines,
            color: [0, 255, 0, 255],
            outline_color: [0, 0, 0, 255],
            outline: 1,
            length: 6,
            thickness: 2,
            gap: 3,
            t_style: false,
            dot: false,
            dot_size: 2,
            circle: false,
            circle_radius: 10,
            circle_thickness: 1,
            grid: 30,
            scale: 1,
            pixels: vec![[0; 4]; 30 * 30],
            image: String::new(),
            image_size: 48,
            image_opacity: 100,
            offset_x: 0,
            offset_y: 0,
        }
    }
}

impl Crosshair {
    /// Resize the pixel grid, keeping the drawing centred.
    pub fn resize_grid(&mut self, n: u32) {
        let n = n.clamp(1, MAX_GRID);
        let old = self.grid as i32;
        let mut px = vec![[0; 4]; (n * n) as usize];
        let d = (n as i32 - old) / 2;
        for y in 0..old {
            for x in 0..old {
                let (nx, ny) = (x + d, y + d);
                if (0..n as i32).contains(&nx)
                    && (0..n as i32).contains(&ny)
                    && let Some(c) = self.pixels.get((y * old + x) as usize)
                {
                    px[(ny * n as i32 + nx) as usize] = *c;
                }
            }
        }
        self.grid = n;
        self.pixels = px;
    }

    /// Repair hand-edited / corrupt values so rendering can't misbehave.
    pub fn sanitize(&mut self) {
        self.scale = self.scale.clamp(1, MAX_SCALE);
        let n = self.grid.clamp(1, MAX_GRID);
        if self.grid != n || self.pixels.len() != (n * n) as usize {
            self.pixels.resize((n * n) as usize, [0; 4]);
            self.grid = n;
        }
        for v in [&mut self.length, &mut self.gap, &mut self.circle_radius] {
            *v = (*v).min(200);
        }
        for v in [
            &mut self.thickness,
            &mut self.outline,
            &mut self.dot_size,
            &mut self.circle_thickness,
        ] {
            *v = (*v).min(50);
        }
        self.image_size = self.image_size.clamp(4, MAX_IMAGE);
        self.image_opacity = self.image_opacity.clamp(5, 100);
        // Only ever a file name inside our images folder, never a path somewhere else.
        self.image = Path::new(&self.image)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
}

/// Virtual-key code + RegisterHotKey modifier bits (1=Alt, 2=Ctrl, 4=Shift).
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Hotkey {
    pub vk: u32,
    pub mods: u32,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub crosshair: Crosshair,
    pub toggle_key: Hotkey,
    pub menu_key: Hotkey,
    pub presets: BTreeMap<String, Crosshair>,
    /// Only show the crosshair while one of `games` is the focused window.
    pub only_games: bool,
    pub games: Vec<Game>,
    pub auto_update: bool,
    pub aim: Aim,
    pub aim_button: AimButton,
    pub aim_toggle: bool, // press once to aim, again to stop (games with toggle ADS)
    pub theme: Theme,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Game {
    pub exe: String,    // e.g. "cs2.exe", matched case-insensitively
    pub name: String,   // what to show, e.g. "Counter-Strike 2"; worked out when it's added
    pub path: String,   // full path when known, for the app's icon
    pub preset: String, // preset to use in this game; empty = current crosshair
    pub windowed: bool, // centre on the game's window instead of its monitor
    /// This game's own aiming rule. None means the default under "While aiming".
    pub aim: Option<AimRule>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            crosshair: Crosshair::default(),
            toggle_key: Hotkey { vk: 0x77, mods: 0 }, // F8
            menu_key: Hotkey { vk: 0x78, mods: 0 },   // F9
            presets: BTreeMap::new(),
            only_games: false,
            games: Vec::new(),
            auto_update: true,
            aim: Aim::Keep,
            aim_button: AimButton::Right,
            aim_toggle: false,
            theme: Theme::System,
        }
    }
}

fn dir(name: &str) -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| ".".into())
        .join(name)
}

fn path() -> PathBuf {
    dir("OpenCrosshair").join("settings.json")
}

/// Where imported crosshair images live.
pub fn images_dir() -> PathBuf {
    dir("OpenCrosshair").join("images")
}

/// Where panics are written, so a crash leaves something for a bug report.
pub fn crash_log() -> PathBuf {
    dir("OpenCrosshair").join("crash.log")
}

/// Copy a picture into our own folder, so the crosshair keeps working if the original is moved
/// or deleted. It's named after its contents, so importing the same file twice keeps one copy.
pub fn import_image(src: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(src)?;
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("png");
    import_bytes(bytes, ext)
}

/// The same as `import_image`, for bytes that didn't come from a file (e.g. a shared preset).
pub fn import_bytes(bytes: Vec<u8>, ext: &str) -> std::io::Result<String> {
    if bytes.len() > 20 << 20 {
        return Err(std::io::Error::other("that file is over 20 MB"));
    }
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x100_0000_01b3) // FNV-1a
    });
    let name = format!("{hash:016x}.{}", ext.to_lowercase());
    std::fs::create_dir_all(images_dir())?;
    std::fs::write(images_dir().join(&name), bytes)?;
    Ok(name)
}

/// Point every aiming rule that switches to `old` at `new`, or at Keep when `new` is None.
fn retarget_aim(aim: &mut Aim, old: &str, new: Option<&str>) {
    if matches!(aim, Aim::Preset(n) if n == old) {
        *aim = new.map_or(Aim::Keep, |n| Aim::Preset(n.into()));
    }
}

impl Settings {
    /// The aiming rule in effect for a game: its own if it has one, otherwise the default.
    pub fn aim_rule_for(&self, game: Option<&Game>) -> AimRule {
        game.and_then(|g| g.aim.clone()).unwrap_or_else(|| AimRule {
            action: self.aim.clone(),
            button: self.aim_button,
            toggle: self.aim_toggle,
        })
    }

    pub fn load() -> Self {
        let p = path();
        // Settings from before the rename.
        let bytes =
            std::fs::read(&p).or_else(|_| std::fs::read(dir("crosshair").join("settings.json")));
        let mut s: Settings = match bytes {
            Ok(b) => serde_json::from_slice(&b).unwrap_or_else(|_| {
                // Don't let the next save silently replace a file we couldn't read.
                let _ = std::fs::rename(&p, p.with_extension("json.bak"));
                Settings::default()
            }),
            Err(_) => Settings::default(),
        };
        s.crosshair.sanitize();
        s.presets.values_mut().for_each(Crosshair::sanitize);
        for g in &mut s.games {
            g.exe = g.exe.trim().to_lowercase();
        }
        s
    }

    pub fn save(&self) {
        let p = path();
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        // Write then rename so a crash mid-write can't corrupt the file.
        let tmp = p.with_extension("tmp");
        if std::fs::write(&tmp, serde_json::to_vec(self).unwrap()).is_ok() {
            let _ = std::fs::rename(tmp, p);
        }
    }

    /// Rename a preset, and point everything that used the old name at the new one.
    pub fn rename_preset(&mut self, old: &str, new: &str) -> Result<(), String> {
        let new = new.trim();
        if new.is_empty() {
            return Err("Give it a name.".into());
        }
        if new == old {
            return Ok(());
        }
        if self.presets.contains_key(new) {
            return Err(format!("There's already a preset called {new}."));
        }
        let Some(crosshair) = self.presets.remove(old) else {
            return Err(format!("There's no preset called {old}."));
        };
        self.presets.insert(new.into(), crosshair);
        retarget_aim(&mut self.aim, old, Some(new));
        for g in &mut self.games {
            if g.preset == old {
                g.preset = new.into();
            }
            if let Some(rule) = &mut g.aim {
                retarget_aim(&mut rule.action, old, Some(new));
            }
        }
        Ok(())
    }

    /// Delete a preset. Anything that used it goes back to the current crosshair, or to Keep.
    pub fn delete_preset(&mut self, name: &str) {
        self.presets.remove(name);
        retarget_aim(&mut self.aim, name, None);
        for g in &mut self.games {
            if g.preset == name {
                g.preset.clear();
            }
            if let Some(rule) = &mut g.aim {
                retarget_aim(&mut rule.action, name, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_can_override_the_default_aiming_rule() {
        let s = Settings {
            aim: Aim::Hide,
            aim_button: AimButton::Middle,
            ..Default::default()
        };
        let mut cs = Game {
            exe: "cs2.exe".into(),
            ..Default::default()
        };

        // No rule of its own: it follows the default, and so does anything that isn't a game.
        assert!(s.aim_rule_for(Some(&cs)).action == Aim::Hide);
        assert!(s.aim_rule_for(None).button == AimButton::Middle);

        // "Right-click does nothing here": the game opts out while the default stays on.
        cs.aim = Some(AimRule::default());
        let rule = s.aim_rule_for(Some(&cs));
        assert!(rule.action == Aim::Keep && rule.button == AimButton::Right);
        assert!(
            s.aim_rule_for(None).action == Aim::Hide,
            "other apps still use the default"
        );
    }

    #[test]
    fn settings_saved_before_per_game_rules_still_load() {
        // Roughly what v0.2.0 wrote: a game with no "aim" field at all.
        let old = r#"{"aim":{"Preset":"Dot"},"games":[{"exe":"cs2.exe","preset":"Classic"}]}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert!(s.games[0].aim.is_none());
        assert!(s.aim_rule_for(Some(&s.games[0])).action == Aim::Preset("Dot".into()));
    }

    /// A preset "Dot", used as the default aim, by a game's crosshair, and by that game's own rule.
    fn dot_everywhere() -> Settings {
        let mut s = Settings {
            aim: Aim::Preset("Dot".into()),
            ..Default::default()
        };
        s.presets.insert("Dot".into(), Crosshair::default());
        s.games.push(Game {
            exe: "cs2.exe".into(),
            preset: "Dot".into(),
            aim: Some(AimRule {
                action: Aim::Preset("Dot".into()),
                ..Default::default()
            }),
            ..Default::default()
        });
        s
    }

    #[test]
    fn renaming_a_preset_repoints_everything_that_used_it() {
        let mut s = dot_everywhere();
        s.presets.insert("Cross".into(), Crosshair::default());

        assert!(s.rename_preset("Dot", "  Pip ").is_ok());
        assert!(s.presets.contains_key("Pip") && !s.presets.contains_key("Dot"));
        assert!(s.aim == Aim::Preset("Pip".into()));
        assert!(s.games[0].preset == "Pip");
        assert!(s.games[0].aim.as_ref().unwrap().action == Aim::Preset("Pip".into()));

        // A name that's taken or empty is refused, and nothing moves.
        assert_eq!(
            s.rename_preset("Pip", "Cross"),
            Err("There's already a preset called Cross.".into())
        );
        assert_eq!(s.rename_preset("Pip", "   "), Err("Give it a name.".into()));
        assert!(s.presets.contains_key("Pip") && s.games[0].preset == "Pip");
    }

    #[test]
    fn deleting_a_preset_resets_everything_that_used_it() {
        let mut s = dot_everywhere();
        s.delete_preset("Dot");

        assert!(s.presets.is_empty());
        assert!(s.aim == Aim::Keep);
        assert!(s.games[0].preset.is_empty());
        assert!(s.games[0].aim.as_ref().unwrap().action == Aim::Keep);
    }
}
