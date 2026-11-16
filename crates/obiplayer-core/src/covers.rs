//! Local cover-art discovery in album folders.

use std::path::{Path, PathBuf};

const EXTENSIONS: [&str; 7] = ["jpg", "jpeg", "png", "bmp", "tiff", "tif", "webp"];
const PREFERRED: [&str; 5] = ["cover", "folder", "front", "artwork", "cover_for_foobar"];
const ART_DIRS: [&str; 8] = [
    "Art", "Artwork", "Scans", "Covers", "art", "artwork", "scans", "covers",
];
/// Sub-folder names that mean "this is one disc of a bigger release".
const DISC_PREFIXES: [&str; 7] = ["disc", "cd", "stereo", "mono", "surround", "side", "disk"];

pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// Probe preferred names directly. Cheaper and more reliable on NFS than
/// listing the directory.
fn preferred_direct(dir: &Path) -> Option<PathBuf> {
    for name in PREFERRED {
        for ext in EXTENSIONS {
            for n in [name.to_owned(), capitalize(name)] {
                let p = dir.join(format!("{n}.{ext}"));
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn images_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| is_image(p) && !hidden(p) && p.is_file())
        .collect();
    v.sort();
    v
}

fn hidden(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
}

fn preferred_in(images: &[PathBuf]) -> Option<PathBuf> {
    images
        .iter()
        .find(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| PREFERRED.contains(&s.to_ascii_lowercase().as_str()))
        })
        .cloned()
}

fn in_art_dirs(dir: &Path) -> Option<PathBuf> {
    for sub in ART_DIRS {
        let d = dir.join(sub);
        if !d.is_dir() {
            continue;
        }
        let imgs = images_in(&d);
        if let Some(p) = preferred_in(&imgs).or_else(|| imgs.into_iter().next()) {
            return Some(p);
        }
    }
    None
}

fn is_disc_folder(dir: &Path) -> bool {
    dir.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|n| DISC_PREFIXES.iter().any(|p| n.starts_with(p)))
}

/// Find the cover image for an album directory.
///
/// Order: preferred names (`cover`, `folder`, `front`, …), any image in the
/// folder, art sub-folders (`Art`, `Scans`, …), then — for `Disc 1`, `CD2`,
/// `Stereo`… folders — the same search up to three parent levels.
pub fn find_cover(album_dir: &Path) -> Option<PathBuf> {
    let dir = std::fs::canonicalize(album_dir).unwrap_or_else(|_| album_dir.to_owned());
    if let Some(p) = preferred_direct(&dir) {
        return Some(p);
    }
    if let Some(p) = images_in(&dir).into_iter().next() {
        return Some(p);
    }
    if let Some(p) = in_art_dirs(&dir) {
        return Some(p);
    }
    let mut cur = dir;
    for _ in 0..3 {
        if !is_disc_folder(&cur) {
            break;
        }
        let Some(parent) = cur.parent().map(Path::to_owned) else {
            break;
        };
        if let Some(p) = preferred_direct(&parent)
            .or_else(|| images_in(&parent).into_iter().next())
            .or_else(|| in_art_dirs(&parent))
        {
            return Some(p);
        }
        cur = parent;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(name: &str) -> Tmp {
            let p = std::env::temp_dir().join(format!("obi-covers-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
        fn touch(&self, rel: &str) -> PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
            std::fs::canonicalize(p).unwrap()
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn prefers_named_cover_over_other_images() {
        let t = Tmp::new("named");
        t.touch("A/back.jpg");
        let want = t.touch("A/Folder.png");
        // Compare case-insensitively: macOS file systems usually are.
        let got = find_cover(&t.0.join("A")).unwrap();
        assert_eq!(
            got.to_string_lossy().to_lowercase(),
            want.to_string_lossy().to_lowercase()
        );
    }

    #[test]
    fn falls_back_to_any_image_then_art_dirs() {
        let t = Tmp::new("any");
        let img = t.touch("A/scan01.jpg");
        assert_eq!(find_cover(&t.0.join("A")), Some(img));

        let front = t.touch("B/Scans/front.jpg");
        t.touch("B/Scans/aaa.jpg");
        t.touch("B/01.flac");
        assert_eq!(find_cover(&t.0.join("B")), Some(front));
    }

    #[test]
    fn disc_folders_look_in_parent() {
        let t = Tmp::new("disc");
        let cover = t.touch("Box/cover.jpg");
        t.touch("Box/Disc 2/01.flac");
        assert_eq!(find_cover(&t.0.join("Box/Disc 2")), Some(cover));
    }

    #[test]
    fn nothing_found() {
        let t = Tmp::new("none");
        t.touch("A/01.flac");
        assert_eq!(find_cover(&t.0.join("A")), None);
        // Non-disc folders never climb to the parent.
        t.touch("cover.jpg");
        assert_eq!(find_cover(&t.0.join("A")), None);
    }
}
