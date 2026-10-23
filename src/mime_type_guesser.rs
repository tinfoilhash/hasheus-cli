use std::path::Path;

// nip-94 requires a mime type, so an unknown file gets the generic one, like the
// web page does when the browser doesn't know it.
pub const DEFAULT_MIME_TYPE: &str = "application/octet-stream";
const MIME_TYPES_BY_EXTENSION: [(&str, &str); 20] = [
    ("7z", "application/x-7z-compressed"),
    ("apk", "application/vnd.android.package-archive"),
    ("appimage", "application/vnd.appimage"),
    ("bz2", "application/x-bzip2"),
    ("deb", "application/vnd.debian.binary-package"),
    ("dmg", "application/x-apple-diskimage"),
    ("exe", "application/vnd.microsoft.portable-executable"),
    ("gz", "application/gzip"),
    ("iso", "application/x-iso9660-image"),
    ("jar", "application/java-archive"),
    ("json", "application/json"),
    ("msi", "application/x-msi"),
    ("pdf", "application/pdf"),
    ("pkg", "application/vnd.apple.installer+xml"),
    ("rpm", "application/x-rpm"),
    ("tar", "application/x-tar"),
    ("txt", "text/plain"),
    ("xz", "application/x-xz"),
    ("zip", "application/zip"),
    ("zst", "application/zstd"),
];

// a guess from the extension, since there's no browser to ask like the web page.
pub fn guess(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase());

    MIME_TYPES_BY_EXTENSION
        .iter()
        .find(|(known_extension, _)| extension.as_deref() == Some(known_extension))
        .map_or(DEFAULT_MIME_TYPE, |(_, mime_type)| mime_type)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_from_the_extension() {
        assert_eq!(
            guess(Path::new("ubuntu.ISO")),
            "application/x-iso9660-image"
        );
        assert_eq!(guess(Path::new("source.tar.gz")), "application/gzip");
    }

    #[test]
    fn falls_back_to_the_generic_mime_type() {
        assert_eq!(guess(Path::new("firmware.bin")), DEFAULT_MIME_TYPE);
        assert_eq!(guess(Path::new("README")), DEFAULT_MIME_TYPE);
    }
}
