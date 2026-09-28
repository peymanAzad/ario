use crate::{aria2::Aria2File, config};
use common::{enums::FileCategory, settings::Settings};
use std::{collections::HashMap, path::Path};

#[derive(Debug)]
enum BencodeValue {
    Bytes(Vec<u8>),
    Int(i64),
    List(Vec<BencodeValue>),
    Dict(HashMap<Vec<u8>, BencodeValue>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileCandidate {
    pub filename: String,
    pub length: u64,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadRoute {
    pub filename: String,
    pub category: FileCategory,
    pub destination_path: String,
}

pub fn route_for_candidates(
    settings: &Settings,
    candidates: &[FileCandidate],
) -> anyhow::Result<DownloadRoute> {
    let representative = candidates.iter().filter(|file| file.selected).fold(
        None,
        |largest: Option<&FileCandidate>, file| {
            if largest.is_none_or(|current| file.length > current.length) {
                Some(file)
            } else {
                largest
            }
        },
    );
    let filename = representative
        .map(|file| file.filename.clone())
        .unwrap_or_default();
    let category = if filename.is_empty() {
        FileCategory::Other
    } else {
        FileCategory::infer_from_filename(&filename, &settings.category_extensions)
    };
    let configured_path = settings
        .category_locations
        .get(&category)
        .unwrap_or(&settings.default_download_location);
    let destination = config::expand_tilde(configured_path)?;
    std::fs::create_dir_all(&destination)?;

    Ok(DownloadRoute {
        filename,
        category,
        destination_path: destination.to_string_lossy().into_owned(),
    })
}

pub fn candidate_from_http_url(url: &str) -> Option<FileCandidate> {
    let url = reqwest::Url::parse(url).ok()?;
    if url.scheme() == "magnet" {
        return None;
    }
    let filename = url
        .path_segments()?
        .next_back()
        .filter(|segment| !segment.is_empty())?;
    Some(FileCandidate {
        filename: filename.to_string(),
        length: 0,
        selected: true,
    })
}

pub fn candidates_from_aria_files(files: &[Aria2File]) -> Vec<FileCandidate> {
    files
        .iter()
        .filter_map(|file| {
            let filename = path_basename(&file.path)?;
            Some(FileCandidate {
                filename,
                length: file.length.parse().unwrap_or(0),
                selected: file.selected.as_deref() != Some("false"),
            })
        })
        .collect()
}

pub fn candidates_from_torrent(data: &[u8]) -> Result<Vec<FileCandidate>, String> {
    let root = BencodeParser::parse(data)?;
    let root = dictionary(&root, "torrent metainfo")?;
    let info = root
        .get(b"info".as_slice())
        .ok_or_else(|| "invalid torrent metainfo: missing info dictionary".to_string())?;
    let info = dictionary(info, "torrent info")?;
    let piece_length = integer(info.get(b"piece length".as_slice()), "piece length")?;
    if piece_length == 0 {
        return Err("invalid torrent metainfo: piece length must be positive".into());
    }
    let pieces = info
        .get(b"pieces".as_slice())
        .and_then(bytes)
        .ok_or_else(|| "invalid torrent metainfo: missing pieces".to_string())?;
    if pieces.len() % 20 != 0 {
        return Err("invalid torrent metainfo: pieces must contain 20-byte hashes".into());
    }

    if let Some(files) = info.get(b"files".as_slice()) {
        let BencodeValue::List(files) = files else {
            return Err("invalid torrent metainfo: files must be a list".into());
        };
        let mut candidates = Vec::with_capacity(files.len());
        for file in files {
            let file = dictionary(file, "torrent file")?;
            let length = integer(file.get(b"length".as_slice()), "file length")?;
            let path = file
                .get(b"path.utf-8".as_slice())
                .or_else(|| file.get(b"path".as_slice()))
                .ok_or_else(|| "invalid torrent metainfo: missing file path".to_string())?;
            let BencodeValue::List(components) = path else {
                return Err("invalid torrent metainfo: file path must be a list".into());
            };
            let filename = components
                .last()
                .and_then(bytes)
                .map(|value| String::from_utf8_lossy(value).into_owned())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "invalid torrent metainfo: empty file path".to_string())?;
            candidates.push(FileCandidate {
                filename,
                length,
                selected: true,
            });
        }
        if candidates.is_empty() {
            return Err("invalid torrent metainfo: torrent contains no files".into());
        }
        validate_piece_count(&candidates, piece_length, pieces)?;
        return Ok(candidates);
    }

    let name = info
        .get(b"name.utf-8".as_slice())
        .or_else(|| info.get(b"name".as_slice()))
        .and_then(bytes)
        .map(|value| String::from_utf8_lossy(value).into_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "invalid torrent metainfo: missing file name".to_string())?;
    let length = integer(info.get(b"length".as_slice()), "file length")?;
    let candidates = vec![FileCandidate {
        filename: name,
        length,
        selected: true,
    }];
    validate_piece_count(&candidates, piece_length, pieces)?;
    Ok(candidates)
}

fn validate_piece_count(
    candidates: &[FileCandidate],
    piece_length: u64,
    pieces: &[u8],
) -> Result<(), String> {
    let total_length = candidates.iter().try_fold(0_u64, |total, file| {
        total.checked_add(file.length).ok_or(())
    });
    let total_length = total_length
        .map_err(|()| "invalid torrent metainfo: total file length is too large".to_string())?;
    let expected = total_length.div_ceil(piece_length);
    if pieces.len() as u64 / 20 != expected {
        return Err("invalid torrent metainfo: piece count does not match file lengths".into());
    }
    Ok(())
}

fn dictionary<'a>(
    value: &'a BencodeValue,
    label: &str,
) -> Result<&'a HashMap<Vec<u8>, BencodeValue>, String> {
    match value {
        BencodeValue::Dict(value) => Ok(value),
        _ => Err(format!(
            "invalid torrent metainfo: {label} must be a dictionary"
        )),
    }
}

fn integer(value: Option<&BencodeValue>, label: &str) -> Result<u64, String> {
    match value {
        Some(BencodeValue::Int(value)) => u64::try_from(*value)
            .map_err(|_| format!("invalid torrent metainfo: {label} must be non-negative")),
        _ => Err(format!("invalid torrent metainfo: missing {label}")),
    }
}

fn bytes(value: &BencodeValue) -> Option<&[u8]> {
    match value {
        BencodeValue::Bytes(value) => Some(value),
        _ => None,
    }
}

struct BencodeParser<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> BencodeParser<'a> {
    fn parse(data: &'a [u8]) -> Result<BencodeValue, String> {
        let mut parser = Self { data, position: 0 };
        let value = parser.value(0)?;
        if parser.position != data.len() {
            return Err("invalid torrent metainfo: trailing bencode data".into());
        }
        Ok(value)
    }

    fn value(&mut self, depth: usize) -> Result<BencodeValue, String> {
        if depth > 64 {
            return Err("invalid torrent metainfo: bencode nesting is too deep".into());
        }
        match self.peek()? {
            b'i' => self.integer(),
            b'l' => self.list(depth + 1),
            b'd' => self.dict(depth + 1),
            b'0'..=b'9' => self.byte_string().map(BencodeValue::Bytes),
            _ => Err("invalid torrent metainfo: invalid bencode value".into()),
        }
    }

    fn integer(&mut self) -> Result<BencodeValue, String> {
        self.position += 1;
        let end = self.data[self.position..]
            .iter()
            .position(|byte| *byte == b'e')
            .map(|offset| self.position + offset)
            .ok_or_else(|| "invalid torrent metainfo: unterminated integer".to_string())?;
        let text = std::str::from_utf8(&self.data[self.position..end])
            .map_err(|_| "invalid torrent metainfo: invalid integer".to_string())?;
        if text.is_empty()
            || text == "-0"
            || (text.starts_with('0') && text.len() > 1)
            || (text.starts_with("-0") && text.len() > 2)
        {
            return Err("invalid torrent metainfo: invalid integer".into());
        }
        let value = text
            .parse::<i64>()
            .map_err(|_| "invalid torrent metainfo: invalid integer".to_string())?;
        self.position = end + 1;
        Ok(BencodeValue::Int(value))
    }

    fn byte_string(&mut self) -> Result<Vec<u8>, String> {
        let colon = self.data[self.position..]
            .iter()
            .position(|byte| *byte == b':')
            .map(|offset| self.position + offset)
            .ok_or_else(|| "invalid torrent metainfo: invalid byte string".to_string())?;
        let text = std::str::from_utf8(&self.data[self.position..colon])
            .map_err(|_| "invalid torrent metainfo: invalid byte string length".to_string())?;
        if text.is_empty() || (text.starts_with('0') && text.len() > 1) {
            return Err("invalid torrent metainfo: invalid byte string length".into());
        }
        let length = text
            .parse::<usize>()
            .map_err(|_| "invalid torrent metainfo: invalid byte string length".to_string())?;
        let start = colon + 1;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| "invalid torrent metainfo: truncated byte string".to_string())?;
        self.position = end;
        Ok(self.data[start..end].to_vec())
    }

    fn list(&mut self, depth: usize) -> Result<BencodeValue, String> {
        self.position += 1;
        let mut values = Vec::new();
        while self.peek()? != b'e' {
            values.push(self.value(depth)?);
        }
        self.position += 1;
        Ok(BencodeValue::List(values))
    }

    fn dict(&mut self, depth: usize) -> Result<BencodeValue, String> {
        self.position += 1;
        let mut values = HashMap::new();
        while self.peek()? != b'e' {
            if !self.peek()?.is_ascii_digit() {
                return Err("invalid torrent metainfo: dictionary key must be bytes".into());
            }
            let key = self.byte_string()?;
            let value = self.value(depth)?;
            values.insert(key, value);
        }
        self.position += 1;
        Ok(BencodeValue::Dict(values))
    }

    fn peek(&self) -> Result<u8, String> {
        self.data
            .get(self.position)
            .copied()
            .ok_or_else(|| "invalid torrent metainfo: unexpected end of bencode data".into())
    }
}

fn path_basename(path: &str) -> Option<String> {
    Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            path.rsplit(['/', '\\'])
                .next()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::settings::Settings;

    #[test]
    fn signed_http_url_uses_only_its_path_for_routing() {
        let candidate =
            candidate_from_http_url("https://example.test/files/movie.MKV?token=abc#ignored")
                .unwrap();
        assert_eq!(candidate.filename, "movie.MKV");
        assert_eq!(
            FileCategory::infer_from_filename(
                &candidate.filename,
                &Settings::default().category_extensions,
            ),
            FileCategory::Video
        );
        assert!(candidate_from_http_url("magnet:?xt=urn:btih:test").is_none());
        let extensionless = candidate_from_http_url("https://example.test/download?id=1").unwrap();
        assert_eq!(
            FileCategory::infer_from_filename(
                &extensionless.filename,
                &Settings::default().category_extensions,
            ),
            FileCategory::Other
        );
    }

    #[test]
    fn largest_selected_file_is_representative_and_ties_keep_input_order() {
        let mut settings = Settings {
            default_download_location: "/tmp/ario-routing-tests".into(),
            ..Settings::default()
        };
        for path in settings.category_locations.values_mut() {
            *path = "/tmp/ario-routing-tests".into();
        }
        settings
            .category_locations
            .insert(FileCategory::Video, "/tmp/ario-routing-tests/Videos".into());
        let candidates = vec![
            FileCandidate {
                filename: "cover.jpg".into(),
                length: 10,
                selected: true,
            },
            FileCandidate {
                filename: "movie.mkv".into(),
                length: 100,
                selected: true,
            },
            FileCandidate {
                filename: "same-size.mp4".into(),
                length: 100,
                selected: true,
            },
            FileCandidate {
                filename: "other.mp4".into(),
                length: 1_000,
                selected: false,
            },
        ];
        let route = route_for_candidates(&settings, &candidates).unwrap();
        assert_eq!(route.filename, "movie.mkv");
        assert_eq!(route.category, FileCategory::Video);
        assert_eq!(route.destination_path, "/tmp/ario-routing-tests/Videos");
    }

    #[test]
    fn parses_single_and_multi_file_torrents() {
        let single = b"d4:infod6:lengthi42e4:name9:movie.mkv12:piece lengthi16384e6:pieces20:00000000000000000000ee";
        assert_eq!(
            candidates_from_torrent(single).unwrap()[0].filename,
            "movie.mkv"
        );

        let multi = b"d4:infod5:filesld6:lengthi10e4:pathl9:cover.jpgeed6:lengthi100e4:pathl9:movie.mkveee4:name6:bundle12:piece lengthi16384e6:pieces20:00000000000000000000ee";
        let candidates = candidates_from_torrent(multi).unwrap();
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[1].filename, "movie.mkv");
    }

    #[test]
    fn rejects_malformed_and_fileless_torrents() {
        assert!(candidates_from_torrent(b"not-bencode").is_err());
        assert!(candidates_from_torrent(b"d4:infod4:name5:emptyee").is_err());
        assert!(candidates_from_torrent(b"d4:infod5:filesle4:name5:emptyee").is_err());
        assert!(
            candidates_from_torrent(
                b"d4:infod6:lengthi42e4:name9:movie.mkv12:piece lengthi16384e6:pieces40:0000000000000000000000000000000000000000ee"
            )
            .is_err()
        );
    }
}
