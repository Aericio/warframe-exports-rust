use fast_image_resize::images::Image;
use fast_image_resize::{PixelType, ResizeOptions, Resizer};
use image::codecs::png::PngEncoder;
use image::ImageEncoder;
use regex::{Captures, Regex};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::error::Error;
use std::io::BufWriter;
use std::path::Path;
use std::sync::LazyLock;
use tokio::fs;

pub static WARFRAME_ORIGIN_URL: &'static str = "https://origin.warframe.com";
pub static WARFRAME_CONTENT_URL: &'static str = "https://content.warframe.com";
pub static LZMA_URL_PATH: &'static str = "/PublicExport/index_en.txt.lzma";
pub static MANIFEST_PATH: &'static str = "/PublicExport/Manifest";
pub static PUBLIC_EXPORT_PATH: &'static str = "/PublicExport";

pub const IMAGE_SIZES: &[u32] = &[256, 128, 64, 32];

pub static RE_ESCAPES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\r\n]").unwrap());
pub static UNWRAP_NONE: LazyLock<String> = LazyLock::new(|| String::from("None"));

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExportManifestItem {
    pub texture_location: String,
    pub unique_name: String,
}

#[allow(non_snake_case)]
#[derive(Deserialize, Debug)]
pub struct ExportManifest {
    pub Manifest: Vec<ExportManifestItem>,
}

/// Configuration for downloading a file.
/// - `url`: The URL of the file to be downloaded.
/// - `path`: The local file path where the downloaded content will be saved.
/// - `name`: The name of the file to be saved.
/// - `as_text`: Whether content should be saved as text or as bytes.
pub struct DownloadConfig {
    pub url: String,
    pub path: String,
    pub name: String,
    pub as_text: bool,
}

/// Struct that holds the extracted resource information.
/// - `name`: The name of the resource.
/// - `hash`: The hash of the resource.
pub struct Resource {
    pub name: String,
    pub hash: String,
}

/// Takes in regex captures and returns an escaped representation of the match.
///
/// # Arguments
/// - `captures` - A `Captures` object from a `Regex::replace_all()` result, expected to match `\r` or `\n`.
///
/// # Returns
/// - A static string: either `"\\r"` if the match is `\r`, or `"\\n"` if the match is `\n`.
/// - `unreachable!()` if an unexpected match occurs, which should never happen given a correct regex.
pub fn escape_match(captures: &Captures) -> &'static str {
    match &captures[0] {
        "\r" => "\\r",
        "\n" => "\\n",
        _ => unreachable!(), // shouldn't happen
    }
}

/// Splits a string into a `Resource` struct containing a name and a hash.
///
/// # Arguments
/// - `string` - A `String` expected to contain a name and a hash, separated by `"!"`.
///
/// # Returns
/// - `Ok(Resource)` - If the string is successfully split into `name` and `hash`.
/// - `panic!` - If the delimiter `"!"` is missing in the input string.
pub fn split_string_to_resource(string: &String) -> Result<Resource, Box<dyn Error>> {
    let Some((name, hash)) = string.split_once("!") else {
        panic!(
            "Attempted to split a resource, but missing hash? ({})",
            string
        )
    };

    Ok(Resource {
        name: name.to_string(),
        hash: hash.to_string(),
    })
}

/// Loads a hash map from a JSON file if it exists; otherwise, returns an empty map.
///
/// # Arguments
/// - `file_path`: Path to the JSON file containing the hash map.
///
/// # Returns
/// - A `BTreeMap` containing the key-value pairs from the JSON file, or an empty map if the file doesn't exist.
pub async fn load_hash_map_from_file(
    file_path: &str,
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    if Path::new(file_path).is_file() {
        let existing_hashes = fs::read_to_string(file_path).await?;
        let map = serde_json::from_str(&existing_hashes)?;
        return Ok(map);
    }

    Ok(BTreeMap::new())
}

/// Resizes an image to the specified square dimensions and encodes it as PNG.
///
/// # Arguments
/// - `src_image` - A reference to the source image to resize.
/// - `size` - The desired output size (width and height, in pixels).
///
/// # Returns
/// - A `Vec<u8>` with PNG-encoded image bytes.
pub async fn resize_image(
    src_image: &Image<'static>,
    size: u32,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut dst_image = Image::new(size, size, PixelType::U8x4);
    let mut resizer = Resizer::new();

    resizer
        .resize(
            &src_image.copy(),
            &mut dst_image,
            &ResizeOptions::new().resize_alg(fast_image_resize::ResizeAlg::Interpolation(
                fast_image_resize::FilterType::Lanczos3,
            )),
        )
        .map_err(|e| format!("Resize failed: {:?}", e))?;

    let mut result_buf = BufWriter::new(Vec::new());
    PngEncoder::new(&mut result_buf)
        .write_image(
            dst_image.buffer(),
            size,
            size,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("Failed to encode image: {}", e))?;

    Ok(result_buf.into_inner().unwrap())
}
