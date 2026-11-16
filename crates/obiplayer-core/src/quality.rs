//! Audio quality score used to rank versions/editions (0–100).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    Best,
    Good,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Score {
    pub dr: u32,
    pub format: u32,
    pub total: u32,
    pub badge: Badge,
}

/// Up to 50 points for dynamic range (DR13+ = 50).
fn dr_score(dr: Option<i32>) -> u32 {
    match dr {
        Some(d) if d > 0 => (d as u32 * 4).min(50),
        _ => 0,
    }
}

/// Up to 50 points for the container format. First match wins.
fn format_score(format: &str) -> u32 {
    let f = format.to_uppercase();
    let has = |xs: &[&str]| xs.iter().any(|x| f.contains(x));
    if has(&["DSF", "DFF", "DSD"]) {
        50
    } else if has(&["FLAC", "WAV", "AIFF", "ALAC"]) {
        40
    } else if has(&["MQA"]) {
        35
    } else if has(&["MP3", "AAC", "OGG", "OPUS"]) {
        10
    } else {
        20
    }
}

pub fn score(dr: Option<i32>, format: &str) -> Score {
    let f = format.to_uppercase();
    let lossy = ["MP3", "AAC", "OGG"].iter().any(|x| f.contains(x));
    let low_dr = matches!(dr, Some(d) if d > 0 && d < 8);
    let (d, fm) = (dr_score(dr), format_score(format));
    Score {
        dr: d,
        format: fm,
        total: d + fm,
        badge: if lossy || low_dr {
            Badge::Warning
        } else {
            Badge::Good
        },
    }
}

/// Score every candidate; all those tied at the top (if > 0) become `Best`.
/// Returns indices into `items` sorted by score, best first.
pub fn rank(items: &[(Option<i32>, &str)]) -> Vec<(usize, Score)> {
    let mut v: Vec<(usize, Score)> = items
        .iter()
        .enumerate()
        .map(|(i, (dr, f))| (i, score(*dr, f)))
        .collect();
    let max = v.iter().map(|(_, s)| s.total).max().unwrap_or(0);
    if max > 0 {
        for (_, s) in &mut v {
            if s.total == max {
                s.badge = Badge::Best;
            }
        }
    }
    v.sort_by_key(|(_, s)| std::cmp::Reverse(s.total));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoring() {
        assert_eq!(score(Some(14), "DSF").total, 100);
        assert_eq!(score(Some(10), "FLAC").total, 80);
        assert_eq!(score(None, "MQA FLAC").format, 40);
        assert_eq!(score(Some(6), "FLAC").badge, Badge::Warning);
        assert_eq!(score(Some(12), "MP3").badge, Badge::Warning);
        assert_eq!(score(Some(12), "OPUS").badge, Badge::Good);
        assert_eq!(score(None, "XYZ").total, 20);
    }

    #[test]
    fn ranking_marks_ties_best() {
        let r = rank(&[(None, "FLAC"), (None, "DSF"), (None, "DSD")]);
        assert_eq!(r[0].1.total, 50);
        assert_eq!(r.iter().filter(|(_, s)| s.badge == Badge::Best).count(), 2);
        assert_eq!(r[2].0, 0);
    }
}
