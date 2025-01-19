use reqwest::Client;
use reqwest::Url;
use std::env;
use std::error::Error;
use std::io::{BufReader, Cursor};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut content_source = env::var("LZMA_CONTENT_SOURCE").expect("Missing LZMA_CONTENT_SOURCE");
    let content_path = "/PublicExport/index_en.txt.lzma";
    content_source.push_str(content_path);

    let url = Url::parse(&content_source)?;

    println!("{}", &content_source);

    let response = Client::new()
        .get(url)
        .header(
            "Authentication",
            env::var("AUTH_TOKEN").expect("Missing AUTH_TOKEN"),
        )
        .send()
        .await?;

    let bytes = response.bytes().await?;
    let cursor = Cursor::new(bytes);

    let mut f = BufReader::new(cursor);
    let mut decomp: Vec<u8> = Vec::new();
    lzma_rs::lzma_decompress(&mut f, &mut decomp)?;

    let out = std::str::from_utf8(&decomp)?;

    println!("{}", &out);

    Ok(())
}
