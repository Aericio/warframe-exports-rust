use regex::Regex;
use reqwest::Client;
use reqwest::Url;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::fs;
use std::io::{BufReader, Cursor};
use std::path::Path;
use std::string::ToString;

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // An HTTP client to share between all requests.
    let client = Client::new();

    // A utility function for unwrapping results with no default value.
    let unwrap_none = "None".to_string();

    // Create missing data folders.
    for folder in STORAGE_FOLDERS {
        if Path::new(folder).is_dir() == false {
            println!("{} directory not found, initializing...", folder);
            fs::create_dir(folder)?;
        }
    }

    let export_indices = fetch_export_indices(&client).await?;
    // println!("{:#?}", export_indices);

    let (export_hashes, updated_hash, updated_manifest) =
        download_exports(&client, export_indices, &unwrap_none).await?;

    if updated_hash {
        let json = serde_json::to_string(&export_hashes)?;
        println!("Wrote hashes to {}", EXPORT_HASH_LOCATION);
        fs::write(EXPORT_HASH_LOCATION, json)?;

        // Now, download images!
        if updated_manifest {
            let mut image_hashes = restore_map(IMAGE_HASH_LOCATION)?;

            let export_manifest =
                fs::read_to_string(format!("{}/{}", STORAGE_FOLDERS[2], "ExportManifest.json"))?;
            let export_manifest: ExportManifest = serde_json::from_str(&export_manifest)?;

            let mut int = 0;
            for ExportManifestItem {
                texture_location,
                unique_name,
            } in export_manifest.Manifest
            {
                if let Some((resource_path, resource_hash)) = texture_location.split_once("!") {
                    if int == 5 {
                        break;
                    } else {
                        int += 1
                    }

                    let cleaned_name = &unique_name.replace("/", ".")[1..];
                    let existing_resource = image_hashes.get(resource_path).unwrap_or(&unwrap_none);

                    // Matching resource was found
                    if existing_resource == resource_hash {
                        continue;
                    }

                    // Got None, meaning a new resource.
                    if *existing_resource == unwrap_none {
                        println!(
                            "Added a new resource ➞ {} ({})",
                            resource_path, resource_hash
                        );
                    } else {
                        // An updated resource was found.
                        println!(
                            "Updated an existing resource ➞ {} ({} from {})",
                            resource_path, resource_hash, existing_resource
                        );
                    }

                    download_file(
                        &client,
                        &format!(
                            "{}{}/{}",
                            WARFRAME_CONTENT_URL,
                            PUBLIC_EXPORT_PATH,
                            &texture_location[1..]
                        ),
                        &format!("{}/{}.png", STORAGE_FOLDERS[1], cleaned_name),
                        false,
                    )
                    .await?;

                    image_hashes.insert(resource_path.to_string(), resource_hash.to_string());
                }
            }

            let json = serde_json::to_string(&image_hashes)?;
            println!("Wrote hashes to {}", IMAGE_HASH_LOCATION);
            fs::write(IMAGE_HASH_LOCATION, json)?;
        } else {
            println!("Manifest was not updated")
        }
    } else {
        println!("No hashes to update!");
    }

    Ok(())
}

async fn download_exports(
    client: &Client,
    export_indices: String,
    unwrap_none: &String,
) -> Result<(BTreeMap<String, String>, bool, bool), Box<dyn Error>> {
    // Check for existing downloads
    let mut export_hashes = restore_map(EXPORT_HASH_LOCATION)?;

    // let re = Regex::new(r"(_en|\.json)").unwrap();
    let mut updated_hash = false;
    let mut updated_manifest = false;
    let mut lines = export_indices.lines();
    while let Some(line) = lines.next() {
        // println!("{:#?}", line);
        if let Some((resource_name, resource_hash)) = line.split_once("!") {
            let existing_resource = export_hashes.get(resource_name).unwrap_or(&unwrap_none);

            // Matching resource was found
            if existing_resource == resource_hash {
                continue;
            }

            updated_hash = true;
            updated_manifest = resource_name == "ExportManifest.json";

            // Got None, meaning a new resource.
            if *existing_resource == *unwrap_none {
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

            download_file(
                &client,
                &format!("{}{}/{}", WARFRAME_CONTENT_URL, MANIFEST_PATH, line),
                &format!("{}/{}", STORAGE_FOLDERS[2], resource_name),
                true,
            )
            .await?;

            export_hashes.insert(resource_name.to_string(), resource_hash.to_string());
        }
    }
    Ok((export_hashes, updated_hash, updated_manifest))
}

fn restore_map(file_path: &str) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    if Path::new(file_path).is_file() {
        let existing_hashes = fs::read_to_string(file_path)?;
        let map = serde_json::from_str(&existing_hashes)?;
        return Ok(map);
    }
    Ok(BTreeMap::new())
}

async fn fetch_export_indices(client: &Client) -> Result<String, Box<dyn Error>> {
    let origin_url = env::var("WARFRAME_ORIGIN_URL").expect("Missing WARFRAME_ORIGIN_URL");
    let lzma_url = format!("{}/{}", origin_url, LZMA_URL_PATH);
    println!("{}", &lzma_url);

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

async fn download_file(
    client: &Client,
    url: &str,
    save_path: &str,
    strip_newlines: bool,
) -> Result<(), Box<dyn Error>> {
    print!("Downloading... {}", url);

    let response = client.get(Url::parse(url)?).send().await?;
    let content = response.text().await?;

    println!("... OK!");
    if strip_newlines {
        let re = Regex::new(r"\\r|\r?\n").unwrap();
        let sanitized_content = re.replace_all(&content, "<NEW_LINE>").to_string();
        fs::write(save_path, sanitized_content)?;
    } else {
        fs::write(save_path, content)?;
    }

    Ok(())
}
