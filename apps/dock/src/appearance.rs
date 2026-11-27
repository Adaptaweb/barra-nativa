//! The theme Settings chose for the dock, the menu bar and their menus and
//! panels. `Auto`, the default, follows Windows' light or dark mode; the
//! others fix both the material and whether content is drawn dark or light.
//!
//! It is process state, like the Windows theme it replaces: everything that
//! draws reads `current()` when it builds, and a change rebuilds through the
//! same keys a Windows theme change does.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use muelle_config::Appearance;
use muelle_shell::system::{transparency_enabled, uses_light_theme};

/// What the glass is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    /// The dock as it always was: the light or dark macOS glass.
    Standard,
    Frosted,
    Clear,
    Aurora,
    /// Liquid glass: clear, with a lens at the edge and a specular rim.
    Liquid,
    /// Liquid glass with a darker tint.
    LiquidTinted,
}

impl Material {
    /// The liquid glass materials, which get the lens and the specular rim.
    pub fn is_liquid(self) -> bool {
        matches!(self, Self::Liquid | Self::LiquidTinted)
    }
}

#[cfg(test)]
impl Material {
    pub const ALL: [Self; 6] = [
        Self::Standard,
        Self::Frosted,
        Self::Clear,
        Self::Aurora,
        Self::Liquid,
        Self::LiquidTinted,
    ];
}

/// What a surface is drawn for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub material: Material,
    /// Light surfaces with dark content; otherwise the reverse.
    pub light: bool,
    /// See `system::transparency_enabled`: without it every material is solid.
    pub glass: bool,
}

static CHOICE: AtomicU8 = AtomicU8::new(Appearance::Auto as u8);
/// The desktop is being streamed (see [`set_streaming`]).
static STREAMING: AtomicBool = AtomicBool::new(false);

pub fn set(choice: Appearance) {
    CHOICE.store(choice as u8, Ordering::Relaxed);
    refresh_streaming();
}

/// Reads again whether the desktop is being streamed; `true` if that changed.
pub fn refresh_streaming() -> bool {
    let on = streaming_marker().is_some_and(|path| path.exists());
    STREAMING.swap(on, Ordering::Relaxed) != on
}

/// The file whose presence says the desktop is being streamed.
fn streaming_marker() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Muelle").join("streaming"))
}

/// `muelle --stream on|off`, for a streaming host such as Sunshine to run
/// when a session starts and ends. Liquid glass keeps the dock's windows out
/// of screen captures, and so out of the stream too; while streaming they
/// use the clear glass instead and stay in captures. The state is kept in a
/// file so a dock started mid-stream knows, and every dock is told to reload.
pub fn set_streaming(on: bool) -> anyhow::Result<()> {
    let marker = anyhow::Context::context(streaming_marker(), "sin LOCALAPPDATA")?;
    if on {
        if let Some(folder) = marker.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(&marker, b"")?;
    } else if marker.exists() {
        std::fs::remove_file(&marker)?;
    }
    muelle_shell::dock_link::notify_config_changed();
    Ok(())
}

const CHOICES: [Appearance; 8] = [
    Appearance::Light,
    Appearance::Dark,
    Appearance::Frosted,
    Appearance::Clear,
    Appearance::Aurora,
    Appearance::Liquid,
    Appearance::LiquidTinted,
    Appearance::Auto,
];

fn decode(stored: u8) -> Appearance {
    CHOICES
        .into_iter()
        .find(|choice| *choice as u8 == stored)
        .unwrap_or_default()
}

#[must_use]
pub fn resolve(choice: Appearance, system_light: bool, glass: bool) -> Look {
    let (material, light) = match choice {
        Appearance::Auto => (Material::Standard, system_light),
        Appearance::Light => (Material::Standard, true),
        Appearance::Dark => (Material::Standard, false),
        Appearance::Frosted => (Material::Frosted, true),
        Appearance::Clear => (Material::Clear, false),
        Appearance::Aurora => (Material::Aurora, false),
        Appearance::Liquid => (Material::Liquid, false),
        Appearance::LiquidTinted => (Material::LiquidTinted, false),
    };
    Look {
        material,
        light,
        glass,
    }
}

#[must_use]
pub fn current() -> Look {
    let choice = decode(CHOICE.load(Ordering::Relaxed));
    // Only `Auto` needs the Windows theme.
    let system_light = choice == Appearance::Auto && uses_light_theme();
    let mut look = resolve(choice, system_light, transparency_enabled());
    // While streaming, liquid glass would be missing from the stream.
    if look.material.is_liquid() && STREAMING.load(Ordering::Relaxed) {
        look.material = Material::Clear;
    }
    look
}

/// Whether menus and labels are drawn light, with dark content.
#[must_use]
pub fn light() -> bool {
    current().light
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_follows_windows_and_the_others_do_not() {
        for system_light in [true, false] {
            assert_eq!(
                resolve(Appearance::Auto, system_light, true).light,
                system_light
            );
            assert!(resolve(Appearance::Light, system_light, true).light);
            assert!(resolve(Appearance::Frosted, system_light, true).light);
            for dark in [
                Appearance::Dark,
                Appearance::Clear,
                Appearance::Aurora,
                Appearance::Liquid,
                Appearance::LiquidTinted,
            ] {
                assert!(!resolve(dark, system_light, true).light);
            }
        }
    }

    #[test]
    fn every_choice_decodes_as_itself() {
        for choice in CHOICES {
            assert_eq!(decode(choice as u8), choice);
        }
    }
}
