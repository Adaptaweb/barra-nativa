//! The user's own wallpapers: pictures and MP4 videos chosen with the Windows
//! file dialog and copied into `%LOCALAPPDATA%\Muelle\Wallpapers\custom`.
//! A picture becomes the Windows wallpaper; a video is played by the dock
//! (`video_wallpaper` in the settings, which the page saves).

use serde::Serialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use windows::{
    Win32::{
        Foundation::HWND,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize,
        },
        UI::Shell::{
            Common::COMDLG_FILTERSPEC, DesktopWallpaper, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM,
            FileOpenDialog, IDesktopWallpaper, IFileOpenDialog, SIGDN_FILESYSPATH,
        },
    },
    core::{HSTRING, PCWSTR, w},
};

const IMAGES: [&str; 6] = ["jpg", "jpeg", "png", "bmp", "webp", "gif"];
const VIDEOS: [&str; 1] = ["mp4"];
/// Largest file accepted, so a mistaken pick cannot fill the disk.
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomWallpaper {
    /// File name inside the custom folder: the id the page sends back.
    name: String,
    /// Full path, saved as `video_wallpaper` for a video.
    path: String,
    video: bool,
    /// A small data URI from the Windows thumbnail, when there is one.
    preview: Option<String>,
}

fn folder() -> Result<PathBuf, String> {
    env::var_os("LOCALAPPDATA")
        .map(|root| PathBuf::from(root).join("Muelle").join("Wallpapers").join("custom"))
        .ok_or_else(|| "No se encontró la carpeta de datos locales".into())
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| IMAGES.contains(&e.as_str()) || VIDEOS.contains(&e.as_str()))
}

/// A name from the page, checked to be a plain file in the custom folder.
fn checked(name: &str) -> Result<PathBuf, String> {
    let path = Path::new(name);
    let plain = path.components().count() == 1
        && path.file_name().is_some_and(|n| n == name)
        && extension(path).is_some();
    if !plain {
        return Err("Nombre de fondo no válido".into());
    }
    let full = folder()?.join(name);
    if full.is_file() { Ok(full) } else { Err("Ese fondo ya no existe".into()) }
}

fn describe(path: &Path) -> Option<CustomWallpaper> {
    let ext = extension(path)?;
    Some(CustomWallpaper {
        name: path.file_name()?.to_str()?.to_owned(),
        path: path.to_string_lossy().into_owned(),
        video: VIDEOS.contains(&ext.as_str()),
        preview: muelle_shell::icons::file_thumbnail(&path.to_string_lossy(), 320)
            .and_then(|image| crate::png_data_url(image.width, image.height, image.bgra)),
    })
}

/// The custom wallpapers, newest first.
#[tauri::command]
pub async fn list_custom_wallpapers() -> Result<Vec<CustomWallpaper>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let _com = Com::new();
        let Ok(entries) = fs::read_dir(folder()?) else {
            return Ok(Vec::new());
        };
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
            .flatten()
            .filter(|e| e.path().is_file() && extension(&e.path()).is_some())
            .map(|e| (e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), e.path()))
            .collect();
        files.sort_by_key(|f| std::cmp::Reverse(f.0));
        Ok(files.iter().filter_map(|(_, p)| describe(p)).collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

struct Com(bool);
impl Com {
    fn new() -> Self {
        // SAFETY: Paired with CoUninitialize in Drop when it succeeded.
        Self(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: Pairs with the successful CoInitializeEx.
            unsafe { CoUninitialize() };
        }
    }
}

fn pick(owner: HWND) -> Result<Option<PathBuf>, String> {
    let _com = Com::new();
    // SAFETY: The dialog, its strings and the returned path live for the calls.
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        let filters = [
            COMDLG_FILTERSPEC {
                pszName: w!("Imágenes y videos"),
                pszSpec: w!("*.jpg;*.jpeg;*.png;*.bmp;*.webp;*.gif;*.mp4"),
            },
            COMDLG_FILTERSPEC { pszName: w!("Videos MP4"), pszSpec: w!("*.mp4") },
            COMDLG_FILTERSPEC { pszName: w!("Imágenes"), pszSpec: w!("*.jpg;*.jpeg;*.png;*.bmp;*.webp;*.gif") },
        ];
        dialog.SetFileTypes(&filters).map_err(|e| e.to_string())?;
        dialog.SetTitle(w!("Agregar fondo de pantalla")).map_err(|e| e.to_string())?;
        let options = dialog.GetOptions().map_err(|e| e.to_string())?;
        dialog
            .SetOptions(options | FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM)
            .map_err(|e| e.to_string())?;
        if dialog.Show(Some(owner)).is_err() {
            return Ok(None); // Cancelled.
        }
        let item = dialog.GetResult().map_err(|e| e.to_string())?;
        let text = item.GetDisplayName(SIGDN_FILESYSPATH).map_err(|e| e.to_string())?;
        let path = text.to_string().map_err(|e| e.to_string());
        CoTaskMemFree(Some(text.0.cast()));
        Ok(Some(PathBuf::from(path?)))
    }
}

/// A free name in `folder` for `source`: its own name, else "name (2).ext".
fn free_name(folder: &Path, source: &Path) -> Option<PathBuf> {
    let stem = source.file_stem()?.to_string_lossy();
    let ext = extension(source)?;
    (1..1000).map(|n| {
        if n == 1 { folder.join(format!("{stem}.{ext}")) } else { folder.join(format!("{stem} ({n}).{ext}")) }
    })
    .find(|candidate| !candidate.exists())
}

/// Opens the Windows file dialog and copies the chosen picture or video into
/// the custom folder. `None` when cancelled.
#[tauri::command]
pub async fn add_custom_wallpaper(window: tauri::WebviewWindow) -> Result<Option<CustomWallpaper>, String> {
    let owner = window.hwnd().map_err(|e| e.to_string())?.0 as isize;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(source) = pick(HWND(owner as _))? else {
            return Ok(None);
        };
        extension(&source).ok_or("Elige una imagen o un video MP4")?;
        let size = fs::metadata(&source).map_err(|e| e.to_string())?.len();
        if size > MAX_BYTES {
            return Err("El archivo pesa más de 2 GB".into());
        }
        let root = folder()?;
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let destination = free_name(&root, &source).ok_or("No se pudo nombrar la copia")?;
        fs::copy(&source, &destination).map_err(|e| e.to_string())?;
        let _com = Com::new();
        Ok(describe(&destination))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Makes a custom picture the Windows wallpaper on every display.
#[tauri::command]
pub async fn apply_custom_picture(name: String) -> Result<(), String> {
    let path = checked(&name)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _com = Com::new();
        // SAFETY: COM call on this thread with a path that lives for it.
        unsafe {
            let desktop: IDesktopWallpaper =
                CoCreateInstance(&DesktopWallpaper, None, CLSCTX_INPROC_SERVER | windows::Win32::System::Com::CLSCTX_LOCAL_SERVER)
                    .map_err(|e| e.to_string())?;
            desktop
                .SetWallpaper(PCWSTR::null(), &HSTRING::from(path.as_os_str()))
                .map_err(|e| e.to_string())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Deletes a custom wallpaper's copy (never the user's original file).
#[tauri::command]
pub fn remove_custom_wallpaper(name: String) -> Result<(), String> {
    fs::remove_file(checked(&name)?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_names_of_known_types_are_accepted() {
        for bad in ["..\\x.jpg", "a/b.jpg", "C:\\x.jpg", "x.exe", "x", ""] {
            assert!(checked(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let root = env::temp_dir().join(format!("barra-custom-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap_or_default();
        let source = Path::new("C:\\Videos\\Nieve.MP4");
        assert_eq!(free_name(&root, source), Some(root.join("Nieve.mp4")));
        fs::write(root.join("Nieve.mp4"), b"x").unwrap_or_default();
        assert_eq!(free_name(&root, source), Some(root.join("Nieve (2).mp4")));
        let _ = fs::remove_dir_all(root);
    }
}
