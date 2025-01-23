use regex::Regex;
use reqwest::Client;
use reqwest::Url;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::io::{BufReader, Cursor};
use std::path::Path;
use std::string::ToString;
use std::sync::Arc;
use std::sync::LazyLock;
use tokio::fs;
use tokio::sync::Mutex;
use tokio::task::JoinSet;

/// Environment Variables
/// WARFRAME_ORIGIN_URL - No trailing slash!
/// PROXY_AUTH_TOKEN

static STORAGE_FOLDERS: [&'static str; 3] = ["./output", "./output/image", "./output/export"];

static EXPORT_HASH_LOCATION: &'static str = "./output/export_hash.json";
static IMAGE_HASH_LOCATION: &'static str = "./output/image_hash.json";

static WARFRAME_CONTENT_URL: &'static str = "https://content.warframe.com";
static LZMA_URL_PATH: &'static str = "/PublicExport/index_en.txt.lzma";
static MANIFEST_PATH: &'static str = "/PublicExport/Manifest";
static PUBLIC_EXPORT_PATH: &'static str = "/PublicExport";

static RE_NEWLINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\r|\r?\n").unwrap());
static UNWRAP_NONE: LazyLock<String> = LazyLock::new(|| String::from("None"));

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ExportManifestItem {
    texture_location: String,
    unique_name: String,
}

#[allow(non_snake_case)]
#[derive(Deserialize, Debug)]
struct ExportManifest {
    Manifest: Vec<ExportManifestItem>,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 5)]
async fn main() -> Result<(), Box<dyn Error>> {
    // An HTTP client to share between all requests.
    let client = Arc::new(Client::new());

    // Create missing data folders.
    for folder in STORAGE_FOLDERS {
        if Path::new(folder).is_dir() == false {
            println!("{} directory not found, initializing...", folder);
            fs::create_dir(folder).await?;
        }
    }

    let mut updated_hash = false;
    let mut updated_manifest = false;

    let mut export_set: JoinSet<()> = JoinSet::new();
    let mut export_hashes = Arc::new(Mutex::new(load_hash_map_from_file(EXPORT_HASH_LOCATION).await?));

    let export_index = download_export_index(&client).await?;
    let mut lines = export_index.lines();
    while let Some(line) = lines.next() {
        let (hash, manifest) = check_and_download_resource(
            &client,
            &mut export_hashes,
            &mut export_set,
            &line.to_string(),
            Arc::new(format!(
                "{}{}/{}",
                WARFRAME_CONTENT_URL, MANIFEST_PATH, line
            )),
            Arc::new(format!(
                "{}/{}",
                STORAGE_FOLDERS[2],
                &line[..(line.len() - 26)]
            )),
            Arc::new(true),
        )
        .await?;

        // Any hash got updated, only set once.
        if hash {
            updated_hash = true;
            // Specifically, Manifest hash was updated.
            if manifest {
                updated_manifest = true;
            }
        }
    }

    // Wait for all downloads to finish...
    export_set.join_all().await;

    if updated_hash {
        let json = serde_json::to_string(&*export_hashes.lock().await)?;
        println!("Saved export hashes ➞ {}", EXPORT_HASH_LOCATION);
        fs::write(EXPORT_HASH_LOCATION, json).await?;

        if updated_manifest {
            let mut image_set = JoinSet::new();
            let mut image_hashes: Arc<Mutex<BTreeMap<String, String>>> =
                Arc::new(Mutex::new(load_hash_map_from_file(IMAGE_HASH_LOCATION).await?));

            let export_manifest: ExportManifest = serde_json::from_str(
                &fs::read_to_string(format!("{}/{}", STORAGE_FOLDERS[2], "ExportManifest.json"))
                    .await?,
            )?;

            for ExportManifestItem {
                texture_location,
                unique_name,
            } in export_manifest.Manifest
            {
                check_and_download_resource(
                    &client,
                    &mut image_hashes,
                    &mut image_set,
                    &texture_location,
                    Arc::new(format!(
                        "{}{}/{}",
                        WARFRAME_CONTENT_URL,
                        PUBLIC_EXPORT_PATH,
                        &texture_location[1..]
                    )),
                    Arc::new(format!(
                        "{}/{}.png",
                        STORAGE_FOLDERS[1],
                        &unique_name.replace("/", ".")[1..]
                    )),
                    Arc::new(false),
                )
                .await?;
            }

            // Wait for all downloads to finish...
            image_set.join_all().await;

            let json = serde_json::to_string(&*image_hashes.lock().await)?;
            println!("Saved image hashes ➞ {}", IMAGE_HASH_LOCATION);
            fs::write(IMAGE_HASH_LOCATION, json).await?;
        } else {
            println!("No changes found in export manifest!")
        }
    } else {
        println!("No exports to update!");
    }

    Ok(())
}


/// Loads a hash map from a JSON file if it exists; otherwise, returns an empty map.
/// 
/// # Arguments
/// - `file_path`: Path to the JSON file containing the hash map.
///
/// # Returns
/// - A `BTreeMap` containing the key-value pairs from the JSON file, or an empty map if the file doesn't exist.
///
/// # Errors
/// - Returns an error if the file cannot be read or the JSON cannot be parsed.
async fn load_hash_map_from_file(file_path: &str) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    if Path::new(file_path).is_file() {
        let existing_hashes = fs::read_to_string(file_path).await?;
        let map = serde_json::from_str(&existing_hashes)?;
        return Ok(map);
    }
    Ok(BTreeMap::new())
}

/// Downloads the export index and decompresses it using LZMA.
/// 
/// # Arguments
/// - `client`: A reference to the HTTP client used for making requests.
/// 
/// # Returns
/// A `Result` containing the decompressed export index as a `String`, or an error.
async fn download_export_index(client: &Client) -> Result<String, Box<dyn Error>> {
    let origin_url = env::var("WARFRAME_ORIGIN_URL").expect("Missing WARFRAME_ORIGIN_URL");
    let lzma_url = format!("{}{}", origin_url, LZMA_URL_PATH);

    let response = client
        .get(Url::parse(&lzma_url)?)
        .header(
            "Authentication",
            env::var("PROXY_AUTH_TOKEN").expect("Missing PROXY_AUTH_TOKEN"),
        )
        .send()
        .await?;

    let bytes = response.bytes().await?;
    let cursor = Cursor::new(bytes);

    let mut reader = BufReader::new(cursor);
    let mut decomp: Vec<u8> = Vec::new();
    lzma_rs::lzma_decompress(&mut reader, &mut decomp)?;
    let out = std::str::from_utf8(&decomp)?;

    Ok(out.to_string())
}

/// Checks if a resource should be downloaded by comparing its hash and initiates the download if necessary.
/// 
/// # Arguments
/// - `client`: Shared HTTP client for making requests.
/// - `hashes`: A shared, thread-safe hash map containing resource hashes.
/// - `join_set`: A set of asynchronous tasks for parallel downloads.
/// - `resource`: Resource descriptor string containing the name and hash.
/// - `download_url`: URL to download the resource from.
/// - `download_path`: Path to save the downloaded file.
/// - `download_as_text`: Flag indicating if the resource should be saved as text (otherwise bytes).
///
/// # Returns
/// - A tuple `(hash_updated, is_manifest)` indicating if the hash was updated and if the resource is a manifest.
///
/// # Errors
/// - Returns an error if the resource parsing fails or if there are issues during task creation.
async fn check_and_download_resource(
    client: &Arc<Client>,
    hashes: &Arc<Mutex<BTreeMap<String, String>>>,
    join_set: &mut JoinSet<()>,
    resource: &String,
    download_url: Arc<String>,
    download_path: Arc<String>,
    download_as_text: Arc<bool>,
) -> Result<(bool, bool), Box<dyn Error>> {
    let Some((resource_name, resource_hash)) = resource.split_once("!") else {
        panic!(
            "Attempted to split a resource, but missing hash? ({})",
            resource
        )
    };

    let hash_lock = hashes.lock().await;
    let existing_resource = hash_lock.get(resource_name).unwrap_or(&UNWRAP_NONE);
    let is_manifest = resource_name == "ExportManifest.json";

    // Matching resource was found, caller should continue.
    if existing_resource == resource_hash {
        return Ok((false, is_manifest));
    }

    // Got None, meaning a new resource.
    if *existing_resource == *UNWRAP_NONE {
        println!(
            "Added a new resource ➞ {} ({})",
            resource_name, resource_hash
        );
    } else {
        // An updated resource was found.
        println!(
            "Updated an existing resource ➞ {} ({} from {})",
            resource_name, resource_hash, existing_resource
        );
    }

    // Frees the lock on hashes
    drop(hash_lock);

    let client = Arc::clone(client);
    let hashes = Arc::clone(hashes);
    let resource_name = resource_name.to_owned();
    let resource_hash = resource_hash.to_owned();
    let download_url = Arc::clone(&download_url);
    let download_path = Arc::clone(&download_path);
    let download_as_text = Arc::clone(&download_as_text);
    join_set.spawn(async move {
        let result = download_file(&client, &download_url, &download_path, *download_as_text).await;
        match result.map_err(|e| e.to_string()) {
            Ok(..) => {
                hashes.lock().await.insert(resource_name, resource_hash);
                ()
            }
            Err(err) => println!(
                "An issue occured while downloading {} ({}): {}",
                resource_name, resource_hash, err
            ),
        }
    });

    Ok((true, is_manifest))
}

/// Downloads a file from a given URL and saves it to a specified path.
/// Optionally processes the content as text by sanitizing newlines.
/// 
/// # Arguments
/// - `client`: HTTP client for making the request.
/// - `url`: URL of the file to download.
/// - `save_path`: Path where the file will be saved.
/// - `as_text`: Flag indicating if the content should be saved as sanitized text.
///
/// # Returns
/// - `Ok(())` if the file is downloaded and saved successfully.
///
/// # Errors
/// - Returns an error if the request, content processing, or file writing fails.
async fn download_file(
    client: &Client,
    url: &str,
    save_path: &str,
    as_text: bool,
) -> Result<(), Box<dyn Error>> {
    // println!("[DOWNLOAD] {}", url);

    let response = client.get(Url::parse(url)?).send().await?;

    if as_text {
        let content = response.text().await?;
        let sanitized_content = RE_NEWLINE.replace_all(&content, "<NEW_LINE>").to_string();
        fs::write(save_path, sanitized_content).await?;
    } else {
        let content = response.bytes().await?;
        fs::write(save_path, content).await?;
    }

    Ok(())
}
