use regex::Regex;
use reqwest::Client;
use reqwest::Url;
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::error::Error;
use std::fs;
use std::io::{BufReader, Cursor};
use std::path::Path;
use std::string::ToString;

/// Environment Variables
/// WARFRAME_CONTENT_URL
/// PROXY_AUTH_TOKEN

static STORAGE_FOLDERS: [&'static str; 3] = ["./output", "./output/image", "./output/export"];

static EXPORT_HASH_LOCATION: &'static str = "./output/export_hash.json";
static IMAGE_HASH_LOCATION: &'static str = "./output/image_hash.json";

static WARFRAME_CONTENT_URL: &'static str = "https://content.warframe.com";
static LZMA_URL_PATH: &'static str = "/PublicExport/index_en.txt.lzma";
static MANIFEST_PATH: &'static str = "/PublicExport/Manifest/";

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

    let mut export_hashes: BTreeMap<String, String> = BTreeMap::new();
    if Path::new(EXPORT_HASH_LOCATION).is_file() {
        let existing_hashes = fs::read_to_string(EXPORT_HASH_LOCATION)?;
        export_hashes = serde_json::from_str(&existing_hashes)?;
        println!("{:#?}", export_hashes);
    }

    // let re = Regex::new(r"(_en|\.json)").unwrap();
    let mut updated_hash = false;
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

            // download json files
            let manifest_url = format!("{}{}{}", WARFRAME_CONTENT_URL, MANIFEST_PATH, line);
            println!("Downloading... {}", &manifest_url);

            let response = client
                .get(Url::parse(&manifest_url)?)
                .send()
                .await?;

            let text = response.text().await?;
            let json = serde_json::to_string(&text)?;

            println!("Wrote file... {}", resource_name);

            fs::write(
                format!("{}/{}", STORAGE_FOLDERS.get(2).unwrap(), resource_name),
                json,
            )?;

            export_hashes.insert(resource_name.to_string(), resource_hash.to_string());
        }
    }

    if updated_hash {
        let json = serde_json::to_string(&export_hashes)?;
        // println!("{:#?}", json);

        println!("Wrote hashes to {}", EXPORT_HASH_LOCATION);
        fs::write(EXPORT_HASH_LOCATION, json)?;
    } else {
        println!("No hashes to update!");
    }

    Ok(())
}

async fn fetch_export_indices(client: &Client) -> Result<String, Box<dyn Error>> {
    let origin_url = env::var("WARFRAME_ORIGIN_URL").expect("Missing WARFRAME_ORIGIN_URL");
    let lzma_url = origin_url.clone() + LZMA_URL_PATH;
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
