//! 导入的宠物形象：Codex 宠物格式（Petdex 等网站用的格式）。
//!
//! 一个形象是一个文件夹：
//!
//! ```text
//! 某个宠物/
//!   pet.json            ← {"id": "...", "displayName": "...", "description": "..."}（都可选）
//!   spritesheet.webp    ← 或 spritesheet.png
//! ```
//!
//! 精灵图是 8 列的图集，每帧 192 × 208（也可以整体等比缩放）。v1 有 9 行，v2 有 11 行（多出的两行
//! 留给各个客户端自用，步步不用）。每一行是一个动作，帧数见 [`STATES`]。
//!
//! 这里只做不需要解码像素的部分：从文件头读出宽高、识别图集版本、解析 pet.json、生成 id。

use serde::{Deserialize, Serialize};

/// 图集列数。
pub const COLUMNS: u32 = 8;
/// 标准帧宽。
pub const FRAME_W: u32 = 192;
/// 标准帧高。
pub const FRAME_H: u32 = 208;
/// 精灵图文件大小上限。
pub const MAX_SHEET_BYTES: usize = 16 * 1024 * 1024;
/// pet.json 大小上限。
pub const MAX_META_BYTES: usize = 64 * 1024;

/// Codex 宠物格式的动作行：(名字, 行号, 帧数, 一轮多少毫秒)。
pub const STATES: &[(&str, u32, u32, u32)] = &[
    ("idle", 0, 6, 1100),
    ("running-right", 1, 8, 1060),
    ("running-left", 2, 8, 1060),
    ("waving", 3, 4, 700),
    ("jumping", 4, 5, 840),
    ("failed", 5, 8, 1220),
    ("waiting", 6, 6, 1010),
    ("running", 7, 6, 820),
    ("review", 8, 6, 1030),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Webp,
}

impl ImageFormat {
    pub fn ext(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Webp => "webp",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Webp => "image/webp",
        }
    }
}

/// 只看文件头，读出图片格式和宽高。不认识的格式返回 None。
pub fn image_size(b: &[u8]) -> Option<(ImageFormat, u32, u32)> {
    let u32be =
        |i: usize| -> Option<u32> { Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?)) };
    let u16le = |i: usize| -> Option<u32> {
        Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?) as u32)
    };
    let u24le = |i: usize| -> Option<u32> {
        let s = b.get(i..i + 3)?;
        Some(s[0] as u32 | (s[1] as u32) << 8 | (s[2] as u32) << 16)
    };
    if b.starts_with(b"\x89PNG\r\n\x1a\n") && b.get(12..16) == Some(b"IHDR") {
        let (w, h) = (u32be(16)?, u32be(20)?);
        return (w > 0 && h > 0).then_some((ImageFormat::Png, w, h));
    }
    if b.get(0..4) == Some(b"RIFF") && b.get(8..12) == Some(b"WEBP") {
        let (w, h) = match b.get(12..16)? {
            b"VP8 " => {
                if b.get(23..26)? != [0x9d, 0x01, 0x2a] {
                    return None;
                }
                (u16le(26)? & 0x3fff, u16le(28)? & 0x3fff)
            }
            b"VP8L" => {
                if *b.get(20)? != 0x2f {
                    return None;
                }
                let bits = u32::from_le_bytes(b.get(21..25)?.try_into().ok()?);
                ((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1)
            }
            b"VP8X" => (u24le(24)? + 1, u24le(27)? + 1),
            _ => return None,
        };
        return (w > 0 && h > 0).then_some((ImageFormat::Webp, w, h));
    }
    None
}

/// 识别出来的图集。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Atlas {
    /// 1（9 行）或 2（11 行）。
    pub version: u8,
    pub rows: u32,
    pub width: u32,
    pub height: u32,
}

/// 按尺寸识别 Codex 图集：8 列 × 9 或 11 行，每格比例 192:208，允许整体等比缩放（格子边界必须是整数像素）。
pub fn detect_atlas(width: u32, height: u32) -> Option<Atlas> {
    for (version, rows) in [(1u8, 9u32), (2, 11)] {
        if width == 0
            || !width.is_multiple_of(COLUMNS)
            || !height.is_multiple_of(rows)
            || (width as u64) * (rows as u64 * FRAME_H as u64)
                != (height as u64) * (COLUMNS as u64 * FRAME_W as u64)
        {
            continue;
        }
        return Some(Atlas {
            version,
            rows,
            width,
            height,
        });
    }
    None
}

/// pet.json 里步步用得到的字段；其他字段原样忽略。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PetMeta {
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
}

pub fn parse_pet_json(text: &str) -> Result<PetMeta, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("pet.json 不是有效的 JSON：{e}"))?;
    let obj = v.as_object().ok_or("pet.json 应该是一个对象")?;
    let s = |k: &str, max: usize| {
        obj.get(k)
            .and_then(|x| x.as_str())
            .map(|x| {
                x.trim()
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(max)
                    .collect::<String>()
            })
            .filter(|x| !x.is_empty())
    };
    Ok(PetMeta {
        id: s("id", 64).or_else(|| s("slug", 64)),
        name: s("displayName", 40).or_else(|| s("name", 40)),
        description: s("description", 200),
    })
}

/// 生成文件夹名用的 id：只保留小写字母、数字和连字符，最长 40 个字符。全是中文等字符时退回 `pet`。
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().chars().flat_map(|c| c.to_lowercase()) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= 40 {
            break;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "pet".into()
    } else {
        out
    }
}

/// 导入的形象 id 是否合法（防止路径穿越）。
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 6, 0, 0, 0]);
        b
    }

    fn riff(chunk: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut b = b"RIFF\0\0\0\0WEBP".to_vec();
        b.extend(chunk);
        b.extend((payload.len() as u32).to_le_bytes());
        b.extend(payload);
        b
    }

    #[test]
    fn reads_png_and_webp_headers() {
        assert_eq!(
            image_size(&png(1536, 1872)),
            Some((ImageFormat::Png, 1536, 1872))
        );
        // 有损 WebP（VP8）：帧标记 3 字节 + 起始码 + 14 位宽高。
        let vp8 = riff(
            b"VP8 ",
            &[0, 0, 0, 0x9d, 0x01, 0x2a, 0x00, 0x06, 0x50, 0x07],
        );
        assert_eq!(image_size(&vp8), Some((ImageFormat::Webp, 1536, 1872)));
        // 无损 WebP（VP8L）：签名 0x2f + (宽-1) 14 位 + (高-1) 14 位。
        let bits: u32 = (1536 - 1) | ((2288 - 1) << 14);
        let mut p = vec![0x2f];
        p.extend(bits.to_le_bytes());
        assert_eq!(
            image_size(&riff(b"VP8L", &p)),
            Some((ImageFormat::Webp, 1536, 2288))
        );
        // 扩展格式（VP8X）：标志 4 字节 + 24 位 (宽-1) + 24 位 (高-1)。
        let mut p = vec![0x10, 0, 0, 0];
        p.extend(&(768u32 - 1).to_le_bytes()[..3]);
        p.extend(&(936u32 - 1).to_le_bytes()[..3]);
        assert_eq!(
            image_size(&riff(b"VP8X", &p)),
            Some((ImageFormat::Webp, 768, 936))
        );
        // 其他格式、截断的文件都不认。
        assert_eq!(image_size(b"GIF89a\x01\0\x01\0"), None);
        assert_eq!(image_size(&png(10, 10)[..18]), None);
        assert_eq!(
            image_size(&riff(b"VP8 ", &[0, 0, 0, 1, 2, 3, 0, 6, 0x50, 7])),
            None
        );
    }

    #[test]
    fn detects_codex_atlases() {
        assert_eq!(
            detect_atlas(1536, 1872).map(|a| (a.version, a.rows)),
            Some((1, 9))
        );
        assert_eq!(
            detect_atlas(1536, 2288).map(|a| (a.version, a.rows)),
            Some((2, 11))
        );
        // 等比缩小一半也行。
        assert_eq!(detect_atlas(768, 936).map(|a| a.version), Some(1));
        // 比例不对、格子不是整数像素的都不行。
        assert!(detect_atlas(1536, 1536).is_none());
        assert!(detect_atlas(100, 117).is_none());
        assert!(detect_atlas(0, 0).is_none());
        // 每个动作都在 v1 的 9 行以内，帧数不超过 8 列。
        assert!(STATES
            .iter()
            .all(|(_, row, frames, ms)| *row < 9 && *frames <= COLUMNS && *ms > 0));
    }

    #[test]
    fn parses_pet_json_leniently() {
        let m = parse_pet_json(r#"{"id":"steve-4","displayName":"Steve","description":"A blocky friend","spriteVersionNumber":1,"x":[1]}"#).unwrap();
        assert_eq!(m.id.as_deref(), Some("steve-4"));
        assert_eq!(m.name.as_deref(), Some("Steve"));
        assert_eq!(m.description.as_deref(), Some("A blocky friend"));
        assert_eq!(parse_pet_json("{}").unwrap(), PetMeta::default());
        assert_eq!(
            parse_pet_json(r#"{"name":"  小\u0007猫 "}"#)
                .unwrap()
                .name
                .as_deref(),
            Some("小猫")
        );
        assert!(parse_pet_json("[1]").is_err());
        assert!(parse_pet_json("nope").is_err());
    }

    #[test]
    fn slugs_are_safe_folder_names() {
        assert_eq!(slugify("Steve 4"), "steve-4");
        assert_eq!(slugify("  ../../etc/passwd "), "etc-passwd");
        assert_eq!(slugify("小猫"), "pet");
        assert_eq!(slugify(&"a".repeat(100)).len(), 40);
        for s in ["Steve 4", "../x", "小猫", "a--b"] {
            assert!(valid_id(&slugify(s)), "{s}");
        }
        assert!(!valid_id("../x"));
        assert!(!valid_id(""));
        assert!(!valid_id("A"));
    }
}
