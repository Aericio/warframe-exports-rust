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
    let mut export_hashes = Arc::new(Mutex::new(restore_map(EXPORT_HASH_LOCATION).await?));

    let export_index = fetch_export_index(&client).await?;
    let mut lines = export_index.lines();
    while let Some(line) = lines.next() {
        let (hash, manifest) = try_download(
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
                Arc::new(Mutex::new(restore_map(IMAGE_HASH_LOCATION).await?));

            let export_manifest: ExportManifest = serde_json::from_str(
                &fs::read_to_string(format!("{}/{}", STORAGE_FOLDERS[2], "ExportManifest.json"))
                    .await?,
            )?;

            for ExportManifestItem {
                texture_location,
                unique_name,
            } in export_manifest.Manifest
            {
                try_download(
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

async fn restore_map(file_path: &str) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    if Path::new(file_path).is_file() {
        let existing_hashes = fs::read_to_string(file_path).await?;
        let map = serde_json::from_str(&existing_hashes)?;
        return Ok(map);
    }
    Ok(BTreeMap::new())
}

async fn fetch_export_index(client: &Client) -> Result<String, Box<dyn Error>> {
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

/// try_download checks whether a resource should be downloaded, and downloads it.
/// Returns (hash_updated, is_manifest)
async fn try_download(
    client: &Arc<Client>,
    hashes: &Arc<Mutex<BTreeMap<String, String>>>,
    join_set: &mut JoinSet<()>,
    resource: &String,
    download_url: Arc<String>,
    download_path: Arc<String>,
    download_as_text: Arc<bool>,
) -> Result<(bool, bool), Box<dyn Error>> {
    let unwrap_none = "None".to_string();

    let Some((resource_name, resource_hash)) = resource.split_once("!") else {
        panic!(
            "Attempted to split a resource, but missing hash? ({})",
            resource
        )
    };

    let hash_lock = hashes.lock().await;
    let existing_resource = hash_lock.get(resource_name).unwrap_or(&unwrap_none);
    let is_manifest = resource_name == "ExportManifest.json";

    // Matching resource was found, caller should continue.
    if existing_resource == resource_hash {
        return Ok((false, is_manifest));
    }

    // Got None, meaning a new resource.
    if *existing_resource == unwrap_none {
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
        let re = Regex::new(r"\\r|\r?\n").unwrap();
        let sanitized_content = re.replace_all(&content, "<NEW_LINE>").to_string();
        fs::write(save_path, sanitized_content).await?;
    } else {
        let content = response.bytes().await?;
        fs::write(save_path, content).await?;
    }

    Ok(())
}
