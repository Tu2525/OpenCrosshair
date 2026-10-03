use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const MAX_GRID: u32 = 64;
pub const MAX_SCALE: u32 = 8;

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum Mode {
    #[default]
    Lines,
    Pixels,
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
    fn sanitize(&mut self) {
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
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Game {
    pub exe: String,    // e.g. "cs2.exe", matched case-insensitively
    pub preset: String, // preset to use in this game; empty = current crosshair
    pub windowed: bool, // centre on the game's window instead of its monitor
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

impl Settings {
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
}
