use regex::Regex;
use reqwest::Client;
use reqwest::Url;
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs;
use std::io::{BufReader, Cursor};
use std::path::Path;

/// Environment Variables
/// WARFRAME_CONTENT_URL
/// PROXY_AUTH_TOKEN

static STORAGE_FOLDERS: [&'static str; 3] = ["./output", "./output/image", "./output/export"];

static EXPORT_HASH_LOCATION: &str = "./output/export_hash.json";
static IMAGE_HASH_LOCATION: &str = "./output/image_hash.json";

static LZMA_URL_PATH: &str = "/PublicExport/index_en.txt.lzma";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let content_url = env::var("WARFRAME_CONTENT_URL").expect("Missing WARFRAME_CONTENT_URL");

    // Create missing data folders
    for folder in STORAGE_FOLDERS {
        if Path::new(folder).is_dir() == false {
            println!("{} directory not found, initializing...", folder);
            fs::create_dir(folder)?;
        }
    }

    let lzma_url = content_url.clone() + LZMA_URL_PATH;
    println!("{}", &lzma_url);

    let response = Client::new()
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

    println!("{:#?}", out);

    let mut export_hashes: HashMap<String, String> = HashMap::new();
    if Path::new(EXPORT_HASH_LOCATION).is_file() {
        let existing_hashes = fs::read_to_string(EXPORT_HASH_LOCATION)?;
        export_hashes = serde_json::from_str(&existing_hashes)?;
        println!("{:#?}", export_hashes);
    }

    let mut update_list: Vec<&str> = Vec::new();

    // let re = Regex::new(r"(_en|\.json)").unwrap();
    let mut lines = out.lines();
    while let Some(line) = lines.next() {
        // println!("{:#?}", line);
        if let Some((resource_name, resource_hash)) = line.split_once("!") {
            let existing_resource = export_hashes.get(resource_name);

            if existing_resource.is_none() {
                println!(
                    "Added a new resource ➞ {} ({})",
                    resource_name, resource_hash
                );
                update_list.push(line);
            } else if existing_resource.unwrap() != resource_hash {
                println!(
                    "Updated an existing resource ➞ {} ({} from {})",
                    resource_name,
                    resource_hash,
                    existing_resource.unwrap()
                );
                update_list.push(line);
            }

            export_hashes.insert(resource_name.to_string(), resource_hash.to_string());
        }
    }

    let json = serde_json::to_string(&export_hashes)?;
    println!("{:#?}", json);

    println!("Wrote hashes to {}", EXPORT_HASH_LOCATION);
    fs::write(EXPORT_HASH_LOCATION, json)?;

    println!("Update list {}", update_list.join(" "));

    Ok(())
}
