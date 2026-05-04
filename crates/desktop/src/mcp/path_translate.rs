//! Translate Windows-style paths to WSL form before sending them to the
//! remote shell. Users on Windows naturally type paths like
//! `C:\Users\me\data\input.fastq` even when the analysis backend is a WSL
//! Ubuntu distribution that needs `/mnt/c/Users/me/data/input.fastq`.
//!
//! The conversion is unconditional: any path that *looks like* a Windows
//! path (`<letter>:\…` or `<letter>:/…`) is rewritten. Paths that already
//! look like Linux paths (`/home/...`, `/mnt/...`, `~/...`, `./...`) pass
//! through untouched, so the function is safe to apply on every tool call
//! regardless of the actual remote OS — a non-Windows path can't
//! accidentally match the Windows pattern.

/// Convert a single path string from Windows form to WSL form.
///
/// Conversion is intentionally conservative: only paths that look like a
/// real absolute Windows path are rewritten. The first byte must be an ASCII
/// letter, the second must be `:`, and the third must be either `\` or `/`
/// (or absent for the bare drive root). This avoids accidentally rewriting
/// unusual but valid Linux strings like `x:test` or `a:b`, which a colon-
/// only check would mis-detect as Windows paths.
///
/// Examples:
///   `C:\Users\me\data` → `/mnt/c/Users/me/data`
///   `D:/foo/bar`       → `/mnt/d/foo/bar`
///   `E:\`              → `/mnt/e`
///   `E:`               → `/mnt/e`
///   `x:test`           → unchanged (no slash after the colon)
///   `/mnt/c/already`   → unchanged
///   `/home/me/data`    → unchanged
///   `~/data`           → unchanged
pub fn windows_to_wsl(path: &str) -> String {
    if path.is_empty() {
        return path.to_string();
    }
    let bytes = path.as_bytes();

    // Require: <letter> ':' [followed by '\' or '/' OR end of string]
    let looks_like_windows_path = bytes.len() >= 2
        && bytes[1] == b':'
        && (bytes[0].is_ascii_uppercase() || bytes[0].is_ascii_lowercase())
        && (bytes.len() == 2 || bytes[2] == b'\\' || bytes[2] == b'/');

    if !looks_like_windows_path {
        return path.to_string();
    }

    let drive = (bytes[0] as char).to_ascii_lowercase();
    let mut rest = path[2..].replace('\\', "/");
    if rest.starts_with('/') {
        rest.remove(0);
    }
    if rest.is_empty() {
        format!("/mnt/{}", drive)
    } else {
        format!("/mnt/{}/{}", drive, rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_uppercase_drive_with_backslash() {
        assert_eq!(windows_to_wsl(r"C:\Users\me"), "/mnt/c/Users/me");
    }

    #[test]
    fn converts_lowercase_drive_with_forward_slash() {
        assert_eq!(windows_to_wsl("d:/foo/bar"), "/mnt/d/foo/bar");
    }

    #[test]
    fn handles_drive_root() {
        assert_eq!(windows_to_wsl("E:\\"), "/mnt/e");
        assert_eq!(windows_to_wsl("E:/"), "/mnt/e");
        assert_eq!(windows_to_wsl("E:"), "/mnt/e");
    }

    #[test]
    fn leaves_linux_paths_alone() {
        assert_eq!(windows_to_wsl("/home/me/data"), "/home/me/data");
        assert_eq!(windows_to_wsl("/mnt/c/Users/me"), "/mnt/c/Users/me");
        assert_eq!(windows_to_wsl("~/data"), "~/data");
        assert_eq!(windows_to_wsl(""), "");
    }

    /// Strings that *look* like a drive letter at first glance but lack the
    /// trailing slash are NOT converted. This protects unusual but valid
    /// Linux usages like `x:test` (e.g., a filename containing a colon, or
    /// a relative path component that happens to start with a letter and
    /// colon).
    #[test]
    fn does_not_convert_letter_colon_without_slash() {
        assert_eq!(windows_to_wsl("x:test"), "x:test");
        assert_eq!(windows_to_wsl("a:b"), "a:b");
        assert_eq!(windows_to_wsl("foo:bar"), "foo:bar");
    }
}
