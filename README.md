# warframe-exports

This is a Rust-based tool for downloading Warframe public export data, like manifests and images, from the Warframe content server.

All exported content is provided as-is, if possible, from the content server. Modifications are listed below:
- `Export*.json` files contain text that include control characters (`\r`, `\n`), and are escaped during pre-processing.
- All images are flattened to the `/image` directory, and use their `unique_name` as the file name with `/` replaced with `.`.

## Outputs

```
output
├── export
│   ├── ExportCustoms_en.json
│   ├── ExportDrones_en.json
│   ├── ...
├── image
│   ├── Lotus.Characters.Tenno.Accessory.Scarves.GrnBannerScarf.GrnBannerScarfItem.png
│   ├── Lotus.Characters.Tenno.Accessory.Scarves.PrimeScarfD.Cloth.PrimeScarfDItem.png
│   ├── ...
├── export_hash.json
└── image_hash.json
```

## Environment Variables

- `OUTPUT_DIRECTORY`: Specify the output directory of the export files (default: `./output`)
- `WARFRAME_ORIGIN_URL`: Specify a custom URL to access warframe origin (default: `https://origin.warframe.com`).
- `PROXY_AUTH_TOKEN`: Specify an authorization token for `WARFRAME_ORIGIN_URL` requests (default: `none`).
