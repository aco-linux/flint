use std::fs::{self, File};
use std::io::{Read, Take};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

use md5::{Digest, Md5};

use crate::item::{Action, Item, Kind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Audio,
    Video,
    Text,
    Document,
    Other,
}

#[derive(Debug, Clone)]
pub enum Preview {
    None,
    Text(String),
    Image(PathBuf),
    Media { path: PathBuf, hint: String },
}

/// Duration / bitrate / tags from one ffmpeg extract. Missing cover is not a failure.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaInfo {
    pub duration: Option<String>,
    pub bitrate: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

impl MediaInfo {
    pub fn is_empty(&self) -> bool {
        self.duration.is_none()
            && self.bitrate.is_none()
            && self.title.is_none()
            && self.artist.is_none()
            && self.album.is_none()
    }

    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(duration) = &self.duration {
            parts.push(duration.clone());
        }
        if let Some(bitrate) = &self.bitrate {
            parts.push(bitrate.clone());
        }
        match (&self.artist, &self.title) {
            (Some(artist), Some(title)) => parts.push(format!("{artist} — {title}")),
            (Some(artist), None) => parts.push(artist.clone()),
            (None, Some(title)) => parts.push(title.clone()),
            (None, None) => {}
        }
        if let Some(album) = &self.album {
            parts.push(album.clone());
        }
        parts.join(" · ")
    }
}

/// Pixels and/or tags from the shared producer.
#[derive(Clone)]
pub struct Produced {
    pub image: Option<DecodedImage>,
    pub info: MediaInfo,
}

pub fn classify(path: &Path) -> MediaKind {
    if path.is_dir() {
        return MediaKind::Other;
    }
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg" | "avif" | "bmp" | "heic" => {
            MediaKind::Image
        }
        "mp3" | "flac" | "wav" | "opus" | "ogg" | "m4a" | "aac" => MediaKind::Audio,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" => MediaKind::Video,
        "md" | "markdown" | "mdx" | "txt" | "rst" | "log" | "json" | "toml" | "yaml" | "yml"
        | "rs" | "py" | "js" | "ts" | "go" | "c" | "h" | "css" | "html" | "sh" => MediaKind::Text,
        "pdf" | "odt" | "doc" | "docx" | "rtf" | "pages" => MediaKind::Document,
        _ => MediaKind::Other,
    }
}

pub fn is_playable(kind: MediaKind) -> bool {
    matches!(kind, MediaKind::Audio | MediaKind::Video)
}

pub fn for_item(item: &Item) -> Preview {
    match item.kind {
        Kind::Weather => {
            if item.subtitle.contains("Detecting") {
                Preview::Text("Looking up the weather for your location…".into())
            } else {
                Preview::Text(format!(
                    "{}\n{}\n\nEnter copies the summary.",
                    item.title, item.subtitle
                ))
            }
        }
        Kind::Calc | Kind::Ai => {
            if item.subtitle.is_empty() {
                Preview::None
            } else {
                Preview::Text(item.subtitle.clone())
            }
        }
        _ => match &item.action {
            Action::OpenPath(path) | Action::PlayMedia { path } => for_path(path),
            Action::Copy(text) if text.chars().count() > 24 => {
                Preview::Text(text.chars().take(800).collect())
            }
            _ => Preview::None,
        },
    }
}

pub fn for_path(path: &Path) -> Preview {
    if !path.exists() {
        return Preview::None;
    }
    if path.is_dir() {
        return Preview::Text(list_dir_preview(path));
    }
    match classify(path) {
        MediaKind::Image => {
            if file_too_heavy(path) {
                Preview::Text("Image is large — Enter opens it in your viewer.".into())
            } else {
                Preview::Image(path.to_path_buf())
            }
        }
        MediaKind::Audio => Preview::Media {
            path: path.to_path_buf(),
            hint: format!(
                "▶  {}\n\nEnter or Space starts playback in your default player.",
                path.display()
            ),
        },
        MediaKind::Video => Preview::Media {
            path: path.to_path_buf(),
            hint: format!("▶  {}\n\nEnter or Space plays this video.", path.display()),
        },
        MediaKind::Text => Preview::Text(read_head(path, 8 * 1024)),
        MediaKind::Document => Preview::Text(format!(
            "{}\n\nEnter opens this document in your editor.",
            path.display()
        )),
        MediaKind::Other => Preview::Text(path.display().to_string()),
    }
}

fn list_dir_preview(path: &Path) -> String {
    let Ok(entries) = fs::read_dir(path) else {
        return path.display().to_string();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    let shown = names.iter().take(16).cloned().collect::<Vec<_>>();
    let more = names.len().saturating_sub(shown.len());
    let mut body = format!("{}\n\n", path.display());
    body.push_str(&shown.join("\n"));
    if more > 0 {
        body.push_str(&format!("\n… {more} more"));
    }
    body
}

pub fn snippet(path: &Path, chars: usize) -> String {
    read_head(path, 8 * 1024).chars().take(chars).collect()
}

pub fn mtime_secs(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Pixel buffer that can cross threads. GdkPixbuf itself is not `Send`.
#[derive(Clone)]
pub struct DecodedImage {
    pub path: PathBuf,
    pub caption: Option<String>,
    width: i32,
    height: i32,
    stride: i32,
    has_alpha: bool,
    pixels: Vec<u8>,
}

impl DecodedImage {
    pub fn is_empty(&self) -> bool {
        self.pixels.is_empty() || self.width <= 0 || self.height <= 0
    }

    pub fn to_pixbuf(&self) -> gdk_pixbuf::Pixbuf {
        gdk_pixbuf::Pixbuf::from_bytes(
            &gtk4::glib::Bytes::from(&self.pixels),
            gdk_pixbuf::Colorspace::Rgb,
            self.has_alpha,
            8,
            self.width,
            self.height,
            self.stride,
        )
    }
}

const LARGE_THUMB_PX: i32 = 256;

pub fn xdg_cache_home() -> PathBuf {
    match std::env::var_os("XDG_CACHE_HOME") {
        Some(home) if !home.is_empty() => PathBuf::from(home),
        _ => dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".cache"),
    }
}

/// Freedesktop Thumbnail Managing Standard: MD5 of the canonical `file://` URI.
pub fn file_uri(path: &Path) -> String {
    let abs = path.canonicalize().unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(path)
        }
    });
    glib::filename_to_uri(&abs, None)
        .map(|uri| uri.to_string())
        .unwrap_or_else(|_| format!("file://{}", abs.display()))
}

pub fn thumb_hash(uri: &str) -> String {
    format!("{:x}", Md5::digest(uri.as_bytes()))
}

pub fn large_thumb_path(cache_home: &Path, hash: &str) -> PathBuf {
    cache_home
        .join("thumbnails")
        .join("large")
        .join(format!("{hash}.png"))
}

/// UI entry: XDG cache home from the environment, then `produce`.
pub fn thumbnail(path: &Path) -> Option<Produced> {
    produce(path, &xdg_cache_home())
}

/// Shared producer: cache first, then image pixbuf or one video/audio extract.
pub fn produce(path: &Path, cache_home: &Path) -> Option<Produced> {
    if crate::files::cancelled() {
        return None;
    }
    let kind = classify(path);
    if !matches!(kind, MediaKind::Image | MediaKind::Video | MediaKind::Audio) {
        return None;
    }
    if let Some(image) = load_cached(path, cache_home) {
        return Some(Produced {
            image: Some(image),
            info: MediaInfo::default(),
        });
    }
    match kind {
        MediaKind::Image => {
            let image = decode_from_file(path)?;
            write_cached(path, cache_home, &image);
            Some(Produced {
                image: Some(image),
                info: MediaInfo::default(),
            })
        }
        MediaKind::Video | MediaKind::Audio => {
            let (pixbuf, info) = extract_frame(path, kind);
            let image = pixbuf.and_then(|pixbuf| {
                let mut image = from_pixbuf(path, &pixbuf)?;
                let summary = info.summary();
                if !summary.is_empty() {
                    image.caption = Some(summary);
                }
                write_cached(path, cache_home, &image);
                Some(image)
            });
            if image.is_none() && info.is_empty() {
                return None;
            }
            Some(Produced { image, info })
        }
        MediaKind::Text | MediaKind::Document | MediaKind::Other => None,
    }
}

fn load_cached(path: &Path, cache_home: &Path) -> Option<DecodedImage> {
    let cached = large_thumb_path(cache_home, &thumb_hash(&file_uri(path)));
    if !cached.is_file() {
        return None;
    }
    if mtime_secs(path) > mtime_secs(&cached) {
        return None;
    }
    let pixbuf = gdk_pixbuf::Pixbuf::from_file(&cached).ok()?;
    from_pixbuf(path, &pixbuf)
}

fn write_cached(path: &Path, cache_home: &Path, image: &DecodedImage) {
    if image.is_empty() {
        return;
    }
    let dir = cache_home.join("thumbnails").join("large");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let hash = thumb_hash(&file_uri(path));
    let dest = large_thumb_path(cache_home, &hash);
    let tmp = dir.join(format!(".{hash}.{}.tmp", std::process::id()));
    let uri = file_uri(path);
    let mtime = mtime_secs(path).to_string();
    let pixbuf = image.to_pixbuf();
    let ok = pixbuf
        .savev(
            &tmp,
            "png",
            &[
                ("tEXt::Thumb::URI", uri.as_str()),
                ("tEXt::Thumb::MTime", mtime.as_str()),
            ],
        )
        .is_ok()
        || pixbuf.savev(&tmp, "png", &[]).is_ok();
    if ok {
        if fs::rename(&tmp, &dest).is_err() {
            let _ = fs::remove_file(&tmp);
        }
    } else {
        let _ = fs::remove_file(&tmp);
    }
}

fn decode_from_file(path: &Path) -> Option<DecodedImage> {
    if file_too_heavy(path) {
        return None;
    }
    let pixbuf = gdk_pixbuf::Pixbuf::from_file(path).ok()?;
    from_pixbuf(path, &pixbuf)
}

fn from_pixbuf(path: &Path, pixbuf: &gdk_pixbuf::Pixbuf) -> Option<DecodedImage> {
    let pixbuf = scale_large(pixbuf);
    let height = pixbuf.height();
    let stride = pixbuf.rowstride();
    let n = (stride as usize).saturating_mul(height.max(0) as usize);
    let bytes = pixbuf.read_pixel_bytes();
    let pixels = bytes.as_ref().get(..n)?.to_vec();
    if pixels.is_empty() {
        return None;
    }
    Some(DecodedImage {
        path: path.to_path_buf(),
        caption: None,
        width: pixbuf.width(),
        height,
        stride,
        has_alpha: pixbuf.has_alpha(),
        pixels,
    })
}

fn scale_large(pixbuf: &gdk_pixbuf::Pixbuf) -> gdk_pixbuf::Pixbuf {
    let width = pixbuf.width();
    let height = pixbuf.height();
    if width <= LARGE_THUMB_PX && height <= LARGE_THUMB_PX {
        return pixbuf.clone();
    }
    let long = width.max(height).max(1);
    let ratio = f64::from(LARGE_THUMB_PX) / f64::from(long);
    let new_w = (f64::from(width) * ratio).round().max(1.0) as i32;
    let new_h = (f64::from(height) * ratio).round().max(1.0) as i32;
    pixbuf
        .scale_simple(new_w, new_h, gdk_pixbuf::InterpType::Bilinear)
        .unwrap_or_else(|| pixbuf.clone())
}

fn extract_frame(path: &Path, kind: MediaKind) -> (Option<gdk_pixbuf::Pixbuf>, MediaInfo) {
    let Some(output) = run_ffmpeg(path, kind, kind == MediaKind::Video) else {
        return (None, MediaInfo::default());
    };
    let info = parse_ffmpeg_info(&String::from_utf8_lossy(&output.stderr));
    let mut pixbuf = pixbuf_from_png_bytes(&output.stdout);
    if pixbuf.is_none()
        && kind == MediaKind::Video
        && let Some(retry) = run_ffmpeg(path, kind, false)
    {
        pixbuf = pixbuf_from_png_bytes(&retry.stdout);
    }
    (pixbuf, info)
}

fn run_ffmpeg(path: &Path, kind: MediaKind, seek: bool) -> Option<std::process::Output> {
    if !bin_on_path("ffmpeg") {
        return None;
    }
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-nostdin", "-hide_banner"]);
    if seek && kind == MediaKind::Video {
        cmd.args(["-ss", "1"]);
    }
    cmd.arg("-i").arg(path);
    cmd.args([
        "-an",
        "-frames:v",
        "1",
        "-f",
        "image2pipe",
        "-vcodec",
        "png",
        "-",
    ]);
    crate::files::run_attached(cmd)
}

fn pixbuf_from_png_bytes(bytes: &[u8]) -> Option<gdk_pixbuf::Pixbuf> {
    if bytes.is_empty() {
        return None;
    }
    gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(bytes.to_vec())).ok()
}

fn bin_on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

/// Parse duration, bitrate, and tags from ffmpeg's stderr. Cover art is separate.
pub fn parse_ffmpeg_info(stderr: &str) -> MediaInfo {
    let mut info = MediaInfo::default();
    for line in stderr.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Duration:") {
            if let Some(duration) = rest.split(',').next() {
                let duration = duration.trim();
                if !duration.is_empty() {
                    info.duration = Some(duration.to_string());
                }
            }
            if let Some(bitrate) = rest.split("bitrate:").nth(1) {
                let bitrate = bitrate.trim();
                if !bitrate.is_empty() && bitrate != "N/A" {
                    info.bitrate = Some(bitrate.to_string());
                }
            }
            continue;
        }
        if let Some((key, value)) = tag_line(line) {
            match key {
                "title" => info.title = Some(value),
                "artist" => info.artist = Some(value),
                "album" => info.album = Some(value),
                _ => {}
            }
        }
    }
    info
}

fn tag_line(line: &str) -> Option<(&str, String)> {
    if !line.starts_with(' ') {
        return None;
    }
    let trimmed = line.trim();
    let (key, value) = trimmed.split_once(':')?;
    let key = key.trim();
    if !matches!(key, "title" | "artist" | "album") {
        return None;
    }
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    Some((key, value.to_string()))
}

/// Read at most `bytes` from the start of the file. Never load the rest.
pub fn read_head(path: &Path, bytes: usize) -> String {
    let Ok(file) = File::open(path) else {
        return path.display().to_string();
    };
    let mut limited: Take<File> = file.take(bytes as u64);
    let mut buf = vec![0u8; bytes];
    let Ok(n) = limited.read(&mut buf) else {
        return path.display().to_string();
    };
    buf.truncate(n);
    if buf.contains(&0) {
        return format!("{} · binary", path.display());
    }
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if n == bytes {
        text.push_str("\n…");
    }
    if text.trim().is_empty() {
        path.display().to_string()
    } else {
        text
    }
}

fn file_too_heavy(path: &Path) -> bool {
    fs::metadata(path)
        .map(|meta| meta.len() > 12 * 1024 * 1024)
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::{
        MediaKind, classify, file_uri, large_thumb_path, parse_ffmpeg_info, produce, thumb_hash,
        thumbnail, xdg_cache_home,
    };
    use gdk_pixbuf::{Colorspace, Pixbuf};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn scratch(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("flint-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_png(path: &Path, width: i32, height: i32, pixel: u32) {
        let pix = Pixbuf::new(Colorspace::Rgb, false, 8, width, height).unwrap();
        pix.fill(pixel);
        pix.savev(path, "png", &[]).unwrap();
    }

    fn ffmpeg_available() -> bool {
        Command::new("ffmpeg")
            .arg("-version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    #[test]
    fn classifies_common_media() {
        assert_eq!(classify(Path::new("song.mp3")), MediaKind::Audio);
        assert_eq!(classify(Path::new("clip.mkv")), MediaKind::Video);
        assert_eq!(classify(Path::new("shot.PNG")), MediaKind::Image);
        assert_eq!(classify(Path::new("notes.md")), MediaKind::Text);
        assert_eq!(classify(Path::new("brief.pdf")), MediaKind::Document);
    }

    #[test]
    fn read_head_does_not_load_the_rest_of_the_file() {
        use super::read_head;
        use std::io::Write;

        let path = std::env::temp_dir().join(format!("flint-head-{}", std::process::id()));
        let mut data = vec![b'a'; 64];
        data.extend_from_slice(b"HEADMARK");
        data.extend(std::iter::repeat_n(b'z', 256 * 1024));
        data.extend_from_slice(b"TAILMARK");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(&data).unwrap();
        drop(file);
        let head = read_head(&path, 72);
        let _ = std::fs::remove_file(&path);
        assert!(
            head.contains("HEADMARK"),
            "expected the start, got {head:?}"
        );
        assert!(
            !head.contains("TAILMARK"),
            "must not read the tail of a large file"
        );
        assert!(head.contains('…'));
    }

    #[test]
    fn freedesktop_uri_hash_and_large_path() {
        let uri = "file:///home/jens/photos/me.png";
        let hash = thumb_hash(uri);
        assert_eq!(hash, "c6ee772d9e49320e97ec29a7eb5b1697");
        let cache = Path::new("/tmp/flint-fake-xdg-cache");
        assert_eq!(
            large_thumb_path(cache, &hash),
            cache
                .join("thumbnails")
                .join("large")
                .join("c6ee772d9e49320e97ec29a7eb5b1697.png")
        );
    }

    #[test]
    fn cache_hit_returns_pixels_without_spawning_extractor() {
        let dir = scratch("thumb-hit");
        let cache = dir.join("cache");
        let source = dir.join("clip.mp4");
        std::fs::write(&source, b"not a real video").unwrap();
        let cached = large_thumb_path(&cache, &thumb_hash(&file_uri(&source)));
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        write_png(&cached, 24, 16, 0x22aa22ff);
        let produced = produce(&source, &cache).expect("cache hit");
        let image = produced.image.expect("planted PNG");
        assert!(!image.is_empty(), "producer must return pixels from cache");
        assert!(image.width > 0 && image.height > 0);
    }

    #[test]
    fn cache_miss_write_back_stores_png_under_hashed_large_path() {
        let dir = scratch("thumb-miss");
        let cache = dir.join("cache");
        let source = dir.join("shot.png");
        write_png(&source, 32, 24, 0xff3300ff);
        let produced = produce(&source, &cache).expect("decode");
        let image = produced.image.expect("pixels from source");
        assert!(!image.is_empty());
        let dest = large_thumb_path(&cache, &thumb_hash(&file_uri(&source)));
        assert!(
            dest.is_file(),
            "miss must write Freedesktop large/ PNG, missing {dest:?}"
        );
        let written = Pixbuf::from_file(&dest).expect("written PNG");
        assert!(written.width() > 0 && written.height() > 0);
    }

    #[test]
    fn thumbnail_wrapper_uses_xdg_cache_home() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = scratch("thumb-xdg");
        let cache = dir.join("xdg");
        let source = dir.join("icon.png");
        write_png(&source, 12, 12, 0x112233ff);
        let old = std::env::var_os("XDG_CACHE_HOME");
        unsafe {
            std::env::set_var("XDG_CACHE_HOME", &cache);
        }
        assert_eq!(xdg_cache_home(), cache);
        let decoded = thumbnail(&source);
        let wrote = large_thumb_path(&cache, &thumb_hash(&file_uri(&source))).is_file();
        match old {
            Some(value) => unsafe { std::env::set_var("XDG_CACHE_HOME", value) },
            None => unsafe { std::env::remove_var("XDG_CACHE_HOME") },
        }
        assert!(decoded.is_some_and(|produced| produced.image.is_some_and(|img| !img.is_empty())));
        assert!(
            wrote,
            "wrapper must write under XDG_CACHE_HOME/thumbnails/large"
        );
    }

    #[test]
    fn cancel_leaves_no_live_extractor_process() {
        let cancel = crate::files::Cancel::new();
        let started = Instant::now();
        let worker = std::thread::spawn({
            let cancel = cancel.clone();
            move || {
                crate::files::with_cancel(cancel, || {
                    let mut cmd = Command::new("sleep");
                    cmd.arg("30");
                    crate::files::run_attached(cmd)
                })
            }
        });
        let deadline = started + Duration::from_secs(2);
        while cancel.attached_pid() == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let pid = cancel.attached_pid();
        assert_ne!(pid, 0, "sleep should have attached before we cancel");
        assert_ne!(pid, std::process::id(), "must not signal our own process");
        cancel.cancel();
        let output = worker.join().unwrap();
        assert!(
            output.is_none(),
            "cancelled extract must not return a completed output"
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "cancel must SIGKILL instead of waiting out sleep 30"
        );
        let still_alive = unsafe { libc::kill(pid as i32, 0) == 0 };
        assert!(!still_alive, "child pid {pid} must not remain after cancel");
        let group_alive = unsafe { libc::kill(-(pid as i32), 0) == 0 };
        assert!(!group_alive, "process group must be gone after cancel");
    }

    #[test]
    fn audio_parser_reads_duration_bitrate_and_tags_without_cover() {
        let stderr = "\
Input #0, mp3, from 'song.mp3':
  Metadata:
    title           : Ping
    artist          : Test Band
    album           : Demos
  Duration: 00:03:14.28, start: 0.000000, bitrate: 320 kb/s
";
        let info = parse_ffmpeg_info(stderr);
        assert_eq!(info.duration.as_deref(), Some("00:03:14.28"));
        assert_eq!(info.bitrate.as_deref(), Some("320 kb/s"));
        assert_eq!(info.title.as_deref(), Some("Ping"));
        assert_eq!(info.artist.as_deref(), Some("Test Band"));
        assert_eq!(info.album.as_deref(), Some("Demos"));
        assert!(
            !info.summary().is_empty(),
            "missing cover must not be a failure"
        );
    }

    #[test]
    fn extracts_frame_from_tiny_video_when_ffmpeg_exists() {
        if !ffmpeg_available() {
            eprintln!("skip video extract: ffmpeg not installed");
            return;
        }
        let dir = scratch("thumb-video");
        let cache = dir.join("cache");
        let video = dir.join("clip.mp4");
        let status = Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=32x32:d=1",
                "-pix_fmt",
                "yuv420p",
                "-y",
            ])
            .arg(&video)
            .status()
            .expect("spawn ffmpeg");
        assert!(status.success(), "could not generate a tiny test video");
        let produced = produce(&video, &cache).expect("video produce");
        let image = produced.image.expect("one extracted frame");
        assert!(!image.is_empty());
        assert!(
            large_thumb_path(&cache, &thumb_hash(&file_uri(&video))).is_file(),
            "video miss must write back to XDG large/"
        );
    }

    #[test]
    fn extracts_audio_tags_when_ffmpeg_exists() {
        if !ffmpeg_available() {
            eprintln!("skip audio extract: ffmpeg not installed");
            return;
        }
        let dir = scratch("thumb-audio");
        let cache = dir.join("cache");
        let audio = dir.join("ping.mp3");
        let status = Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-metadata",
                "title=Ping",
                "-metadata",
                "artist=Flint",
                "-f",
                "mp3",
                "-y",
            ])
            .arg(&audio)
            .status()
            .expect("spawn ffmpeg");
        assert!(status.success(), "could not generate a tiny test mp3");
        let produced = produce(&audio, &cache).expect("audio produce");
        assert!(
            produced.image.is_none(),
            "fixture has no cover art; that is not a failure"
        );
        assert!(
            produced.info.duration.is_some()
                || produced.info.bitrate.is_some()
                || produced.info.title.as_deref() == Some("Ping")
                || produced.info.artist.as_deref() == Some("Flint"),
            "expected duration, bitrate, or a tag, got {:?}",
            produced.info
        );
    }
}
