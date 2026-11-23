//! A chart saved as a file: as SVG (the text written), or as PNG (the SVG rasterized,
//! its texts in the system's fonts), written whole or not at all. Work for a worker.

use std::path::Path;

/// `svg` written to `path`: as it is for an `.svg`, rasterized to PNG otherwise. The file's
/// name on success, why not otherwise.
pub fn write(path: &Path, svg: &str) -> Result<String, String> {
    let as_svg =
        path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    let bytes = if as_svg {
        svg.as_bytes().to_vec()
    } else {
        let image = image_decode::decode_svg(svg.as_bytes()).map_err(|e| e.to_string())?;
        image_decode::encode_png(image.width, image.height, image.pixels)
            .map_err(|e| e.to_string())?
    };
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&temp, &bytes).and_then(|()| std::fs::rename(&temp, path)).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        e.to_string()
    })?;
    Ok(kernel::paths::file_name(path))
}

#[cfg(test)]
#[path = "../tests/unit/export.rs"]
mod tests;
