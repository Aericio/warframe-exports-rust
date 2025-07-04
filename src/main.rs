use fast_image_resize::images::Image;
use fast_image_resize::PixelType;
use image::ImageReader;
use reqwest::Client;
use reqwest::Url;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
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

use warframe_exports::{
    // Functions
    escape_match,
    load_hash_map_from_file,
    resize_image,
    split_string_to_resource,

    // Structs
    DownloadConfig,
    ExportManifest,
    ExportManifestItem,
    Resource,

    // Constants
    IMAGE_SIZES,
    LZMA_URL_PATH,
    MANIFEST_PATH,
    PUBLIC_EXPORT_PATH,
    RE_ESCAPES,
    UNWRAP_NONE,
    WARFRAME_CONTENT_URL,
    WARFRAME_ORIGIN_URL,
};

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    // An HTTP client to share between all requests.
    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
    let client = Arc::new(
        ClientBuilder::new(Client::new())
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build(),
    );

    // Create output directory.
    let output_dir = env::var("OUTPUT_DIRECTORY").unwrap_or("./output".to_string());

    let storage_folders = [
        format!("{}/", output_dir),
        format!("{}/image", output_dir),
        format!("{}/export", output_dir),
    ];

    let export_hash_location = format!("{}/export_hash.json", output_dir);
    let image_hash_location = format!("{}/image_hash.json", output_dir);

    // Create missing data folders.
    for folder in &storage_folders {
        if Path::new(folder).is_dir() == false {
            println!("{} directory not found, initializing...", folder);
            fs::create_dir(folder).await?;
        }
    }

    // Create missing resize-directory data folders.
    for size in IMAGE_SIZES {
        let folder = format!("{}/{}x{}", &storage_folders[1], size, size);
        if Path::new(&folder).is_dir() == false {
            println!("{} directory not found, initializing...", folder);
            fs::create_dir(folder).await?;
        }
    }

    let mut updated_hash = false;
    let mut updated_manifest = false;

    let mut export_set: JoinSet<()> = JoinSet::new();
    let mut export_hashes = Arc::new(Mutex::new(
        load_hash_map_from_file(&export_hash_location).await?,
    ));

    let export_index = download_export_index(&client).await?;
    let mut lines = export_index.lines();
    while let Some(line) = lines.next() {
        let (hash, manifest) = check_and_download_resource(
            &client,
            &mut export_hashes,
            &mut export_set,
            Arc::new(split_string_to_resource(&line.to_string())?),
            Arc::new(DownloadConfig {
                url: format!("{}{}/{}", WARFRAME_CONTENT_URL, MANIFEST_PATH, line),
                path: storage_folders[2].clone(),
                // Remove the last 31 characters, which is the ".json!" plus the 25-digit hash.
                name: line[..(line.len() - 31)].to_string(),
                as_text: true,
            }),
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
        println!("Saved export hashes ➞ {}", export_hash_location);
        fs::write(&export_hash_location, json).await?;

        if updated_manifest {
            let mut image_set = JoinSet::new();
            let mut image_hashes: Arc<Mutex<BTreeMap<String, String>>> = Arc::new(Mutex::new(
                load_hash_map_from_file(&image_hash_location).await?,
            ));

            let export_manifest: ExportManifest = serde_json::from_str(
                &fs::read_to_string(format!("{}/{}", &storage_folders[2], "ExportManifest.json"))
                    .await?,
            )?;

            for ExportManifestItem {
                texture_location,
                unique_name,
            } in export_manifest.Manifest
            {
                let resource = split_string_to_resource(&texture_location)?;

                check_and_download_resource(
                    &client,
                    &mut image_hashes,
                    &mut image_set,
                    Arc::new(Resource {
                        name: unique_name.clone(),
                        hash: resource.hash,
                    }),
                    Arc::new(DownloadConfig {
                        url: format!(
                            "{}{}{}",
                            WARFRAME_CONTENT_URL, PUBLIC_EXPORT_PATH, &texture_location
                        ),
                        path: storage_folders[1].clone(),
                        name: format!("{}.png", &unique_name.replace("/", ".")[1..]),
                        as_text: false,
                    }),
                )
                .await?;
            }

            // Wait for all downloads to finish...
            image_set.join_all().await;

            let json = serde_json::to_string(&*image_hashes.lock().await)?;
            println!("Saved image hashes ➞ {}", &image_hash_location);
            fs::write(&image_hash_location, json).await?;
        } else {
            println!("No changes found in export manifest!")
        }
    } else {
        println!("No exports to update!");
    }

    Ok(())
}

/// Downloads the export index and decompresses it using LZMA.
///
/// # Arguments
/// - `client`: A reference to the HTTP client used for making requests.
///
/// # Returns
/// A `Result` containing the decompressed export index as a `String`, or an error.
async fn download_export_index(client: &ClientWithMiddleware) -> Result<String, Box<dyn Error>> {
    let origin_url = env::var("WARFRAME_ORIGIN_URL").unwrap_or(WARFRAME_ORIGIN_URL.to_string());
    let lzma_url = format!("{}{}", origin_url, LZMA_URL_PATH);

    let response = client
        .get(Url::parse(&lzma_url)?)
        .header(
            "X-Proxy-Token",
            env::var("X_PROXY_TOKEN").unwrap_or_default(),
        )
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to download export index: {}",
            response.status()
        )
        .into());
    }

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
/// - `hashes`: Shared hash map containing resource hashes.
/// - `join_set`: A set of asynchronous tasks for parallel downloads.
/// - `resource`: Resource descriptor string containing the name and hash.
/// - `download_config`: Struct that specifies the download configuration.
///
/// # Returns
/// - A tuple `(hash_updated, is_manifest)` indicating if the hash was updated and if the resource is a manifest.
async fn check_and_download_resource(
    client: &Arc<ClientWithMiddleware>,
    hashes: &Arc<Mutex<BTreeMap<String, String>>>,
    join_set: &mut JoinSet<()>,
    resource: Arc<Resource>,
    download_config: Arc<DownloadConfig>,
) -> Result<(bool, bool), Box<dyn Error>> {
    let hash_lock = hashes.lock().await;
    let existing_resource = hash_lock.get(&resource.name).unwrap_or(&UNWRAP_NONE);
    let is_manifest = resource.name == "ExportManifest.json";

    // Matching resource was found, caller should continue.
    if *existing_resource == resource.hash {
        return Ok((false, is_manifest));
    }

    // Got None, meaning a new resource.
    if *existing_resource == *UNWRAP_NONE {
        println!(
            "Added a new resource ➞ {} ({})",
            resource.name, resource.hash
        );
    } else {
        // An updated resource was found.
        println!(
            "Updated an existing resource ➞ {} ({} from {})",
            resource.name, resource.hash, existing_resource
        );
    }

    // Frees the lock on hashes
    drop(hash_lock);

    let client = Arc::clone(client);
    let hashes = Arc::clone(hashes);
    let download_config = Arc::clone(&download_config);
    join_set.spawn(async move {
        let result = download_file(&client, download_config).await;
        match result.map_err(|e| e.to_string()) {
            Ok(..) => {
                hashes
                    .lock()
                    .await
                    .insert(resource.name.to_owned(), resource.hash.to_owned());
                ()
            }
            Err(err) => println!(
                "An issue occurred while downloading {} ({}): {}",
                resource.name, resource.hash, err
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
/// - `download_config`: Struct that specifies the download configuration.
///
/// # Returns
/// - `Ok(())` if the file is downloaded and saved successfully.
async fn download_file(
    client: &ClientWithMiddleware,
    download_config: Arc<DownloadConfig>,
) -> Result<(), Box<dyn Error>> {
    let response = client.get(Url::parse(&download_config.url)?).send().await?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to download {}: {}",
            download_config.name,
            response.status()
        )
        .into());
    }

    if download_config.as_text {
        let content = response.text().await?;
        let sanitized = RE_ESCAPES.replace_all(&content, escape_match).to_string();
        let parsed_json: serde_json::Value = serde_json::from_str(&sanitized)?;

        fs::write(
            format!(
                "{}/{}.min.json",
                &download_config.path, &download_config.name
            ),
            serde_json::to_string(&parsed_json)?,
        )
        .await?;
        fs::write(
            format!("{}/{}.json", &download_config.path, &download_config.name),
            serde_json::to_string_pretty(&parsed_json)?,
        )
        .await?;

        println!("[DOWNLOADED] ➞ {}", download_config.name);
    } else {
        let content = response.bytes().await?;
        let reader = ImageReader::new(Cursor::new(&content)).with_guessed_format()?;

        if let Ok(decoded) = reader.decode() {
            let rgba_image = decoded.to_rgba8();
            let (width, height) = rgba_image.dimensions();

            let raw_image =
                Image::from_vec_u8(width, height, rgba_image.into_raw(), PixelType::U8x4)?;

            // Save the original image, but constrain to 512x512.
            //  Some are originally over this size, while some are originally under.
            let original_path = format!("{}/{}", &download_config.path, &download_config.name);
            if width == 512 && height == 512 {
                fs::write(&original_path, &content).await?;
            } else {
                let resized_buf = resize_image(&raw_image, 512).await?;
                fs::write(&original_path, resized_buf).await?;
            }

            for size in IMAGE_SIZES {
                let resized_buf = resize_image(&raw_image, *size).await?;
                fs::write(
                    format!(
                        "{}/{}x{}/{}",
                        &download_config.path, size, size, &download_config.name
                    ),
                    resized_buf,
                )
                .await?;
            }

            println!("[DOWNLOADED] ➞ {}", download_config.name);
        } else {
            return Err("Invalid or corrupt image format".into());
        }
    }

    Ok(())
}
