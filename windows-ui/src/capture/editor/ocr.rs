//! Windows Runtime OCR for the text-aware highlighter.
//!
//! `Windows.Media.Ocr` ships with the OS, so detection needs no model
//! downloads. Rust cannot call WinRT async APIs without pulling the `windows`
//! crate's async plumbing in, so a short PowerShell helper does the
//! recognition: it reads the screenshot from a unique temp PNG (the path
//! travels in an environment variable, never interpolated into the script)
//! and prints each word's bounding rect plus a line separator on stdout.

use image::RgbaImage;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// One recognised line: the words that compose it, in reading order.
#[derive(Debug, Clone)]
pub struct OcrLine {
    /// Word bounding rectangles in image pixels (x, y, width, height).
    pub words: Vec<(i32, i32, i32, i32)>,
}

/// The PowerShell helper. Prints one `x,y,w,h` row per recognised word and a
/// `---` row at the end of each text line. Exit code 2 means no recognizer is
/// installed; other failures exit non-zero with the error on stderr.
const OCR_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
try {
    $path = $env:APEXSHOT_OCR_IMAGE
    if (-not $path) { exit 3 }
    [void][Windows.Storage.StorageFile, Windows.Storage, ContentType = WindowsRuntime]
    [void][Windows.Graphics.Imaging.BitmapDecoder, Windows.Graphics.Imaging, ContentType = WindowsRuntime]
    [void][Windows.Media.Ocr.OcrEngine, Windows.Media.Ocr, ContentType = WindowsRuntime]
    Add-Type -AssemblyName System.Runtime.WindowsRuntime
    $asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
        $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and
        $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
    $asTaskAction = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
        $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and
        $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncAction' })[0]
    function Await($op, [type]$t) {
        $task = $asTask.MakeGenericMethod($t).Invoke($null, @($op))
        $task.Wait()
        $task.Result
    }
    function AwaitAction($op) {
        $asTaskAction.Invoke($null, @($op)).Wait()
    }
    $file = Await ([Windows.Storage.StorageFile]::GetFileFromPathAsync($path)) ([Windows.Storage.StorageFile])
    # OpenReadAsync returns IRandomAccessStreamWithContentType; Await's
    # generic must match the operation's result type exactly.
    $stream = Await ($file.OpenReadAsync()) ([Windows.Storage.Streams.IRandomAccessStreamWithContentType])
    $decoder = Await ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
    $engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()
    if ($null -eq $engine) { exit 2 }
    $bitmap = Await ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
    # WinRT OCR refuses images larger than MaximumImageDimension. Decoders
    # do not accept scaling, so a big frame is re-encoded through a
    # BitmapEncoder transform and decoded back at the smaller size.
    # MaxImageDimension is a static engine limit, not an instance member.
    $maxDim = [double][Windows.Media.Ocr.OcrEngine]::MaxImageDimension
    if ($maxDim -le 0) { $maxDim = [double]::MaxValue }
    $w = [double]$bitmap.PixelWidth
    $h = [double]$bitmap.PixelHeight
    $scale = [math]::Min(1.0, $maxDim / [math]::Max($w, $h))
    if ($scale -lt 1.0) {
        $mem = New-Object Windows.Storage.Streams.InMemoryRandomAccessStream
        $encoder = Await ([Windows.Graphics.Imaging.BitmapEncoder]::CreateAsync(
            [Windows.Graphics.Imaging.BitmapEncoder]::PngEncoderId, $mem)) (
            [Windows.Graphics.Imaging.BitmapEncoder])
        $encoder.SetSoftwareBitmap($bitmap)
        $encoder.BitmapTransform.ScaledWidth = [uint32][math]::Floor($w * $scale)
        $encoder.BitmapTransform.ScaledHeight = [uint32][math]::Floor($h * $scale)
        AwaitAction ($encoder.FlushAsync())
        $small = Await ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($mem)) (
            [Windows.Graphics.Imaging.BitmapDecoder])
        $bitmap = Await ($small.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
        $mem.Dispose()
    }
    # Words come back in (possibly) downscaled coordinates: map them back to
    # the original pixel space the annotation model uses.
    $inv = 1.0 / $scale
    if ($bitmap.PixelFormat -ne [Windows.Graphics.Imaging.BitmapPixelFormat]::Bgra8) {
        $bitmap = [Windows.Graphics.Imaging.SoftwareBitmap]::Convert($bitmap, [Windows.Graphics.Imaging.BitmapPixelFormat]::Bgra8)
    }
    $result = Await ($engine.RecognizeAsync($bitmap)) ([Windows.Media.Ocr.OcrResult])
    foreach ($line in $result.Lines) {
        foreach ($word in $line.Words) {
            $r = $word.BoundingRect
            '{0},{1},{2},{3}' -f [int][math]::Floor($r.X * $inv), [int][math]::Floor($r.Y * $inv), [int][math]::Ceiling($r.Width * $inv), [int][math]::Ceiling($r.Height * $inv)
        }
        '---'
    }
    $stream.Dispose()
    exit 0
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
"#;

fn temp_png_path() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("apexshot-ocr-{}-{stamp}.png", std::process::id()))
}

/// Recognize word rectangles in `image`. Returns lines of words in reading
/// order; an empty image or image with no text yields an empty list. Errors
/// when the helper cannot run or no recognizer is installed.
pub fn detect_word_lines(image: &RgbaImage) -> Result<Vec<OcrLine>, String> {
    if image.width() == 0 || image.height() == 0 {
        return Ok(Vec::new());
    }
    let path = temp_png_path();
    image
        .save_with_format(&path, image::ImageFormat::Png)
        .map_err(|error| format!("could not write the OCR working image: {error}"))?;
    let result = run_helper(&path);
    // Only the temp file this call created is removed.
    let _ = std::fs::remove_file(&path);
    result
}

fn run_helper(path: &PathBuf) -> Result<Vec<OcrLine>, String> {
    let output = crate::utils::hidden_command("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            OCR_SCRIPT,
        ])
        .env("APEXSHOT_OCR_IMAGE", path)
        .output()
        .map_err(|error| format!("could not start the Windows OCR helper: {error}"))?;

    match output.status.code() {
        Some(0) => Ok(parse_output(&String::from_utf8_lossy(&output.stdout))),
        Some(2) => Err(
            "Windows text recognition is not installed. Add an OCR language pack in Windows Settings."
                .to_string(),
        ),
        _ => {
            let detail = String::from_utf8_lossy(&output.stderr);
            Err(format!(
                "Windows text recognition failed: {}",
                detail.trim()
            ))
        }
    }
}

fn parse_output(stdout: &str) -> Vec<OcrLine> {
    let mut lines = Vec::new();
    let mut words = Vec::new();
    for row in stdout.lines().map(str::trim) {
        if row == "---" {
            if !words.is_empty() {
                lines.push(OcrLine {
                    words: std::mem::take(&mut words),
                });
            }
            continue;
        }
        let mut parts = row.split(',');
        let (Some(x), Some(y), Some(w), Some(h)) = (
            parts.next().and_then(|v| v.parse().ok()),
            parts.next().and_then(|v| v.parse().ok()),
            parts.next().and_then(|v| v.parse().ok()),
            parts.next().and_then(|v| v.parse().ok()),
        ) else {
            continue;
        };
        if w > 0 && h > 0 {
            words.push((x, y, w, h));
        }
    }
    if !words.is_empty() {
        lines.push(OcrLine { words });
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{parse_output, OcrLine};

    fn line(words: &[(i32, i32, i32, i32)]) -> OcrLine {
        OcrLine {
            words: words.to_vec(),
        }
    }

    #[test]
    fn parse_output_groups_words_into_lines() {
        let out = "1,2,3,4\n5,2,3,4\n---\n1,10,3,4\n---\n";
        let lines = parse_output(out);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].words, vec![(1, 2, 3, 4), (5, 2, 3, 4)]);
        assert_eq!(lines[1].words, vec![(1, 10, 3, 4)]);
    }

    #[test]
    fn parse_output_ignores_malformed_and_empty_rows() {
        assert!(parse_output("junk\n,,\n--\n").is_empty() || parse_output("junk\n").is_empty());
        assert_eq!(parse_output("0,0,0,4\n---\n").len(), 0);
        let _ = line(&[]);
    }
}
