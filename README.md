# warframe-exports [![](https://data.jsdelivr.com/v1/package/gh/aericio/warframe-exports-data/badge)](https://www.jsdelivr.com/package/gh/aericio/warframe-exports-data)

This is a Rust-based tool for downloading Warframe public export data, like manifests and images, from the Warframe content server.

All exported content is provided as-is, if possible, from the content server. Modifications are listed below:
- `Export*.json` / `Export*.min.json` files contain text that include control characters (`\r`, `\n`), and are escaped during pre-processing.
- All images are flattened to the `/image` directory, and use their `unique_name` as the file name with `/` replaced with `.`.
- Downscaled versions of each image are stored in subfolders within `/image`, in the sizes `256x256`, `128x128`, `64x64`, and `32x32`.
  - Images in the root `/image` directory are rescaled to 512x512, if needed, for consistency; some images were originally smaller (e.g. `128x128`) or larger (e.g. `2048x2048`).
  - Scaling is performed using Lancozs3 interpolation.

> ![NOTE]
> [warframe-exports-data](https://github.com/Aericio/warframe-exports-data/) repository runs this tool hourly on weekdays, providing a fully pre-exported snapshot of all available content. You can easily use the exports in your own app through the jsDelivr CDN. Visit the data repository for more information.

## Outputs

```
output/
├── export/
│   ├── ExportCustoms_en.json
│   ├── ExportCustoms_en.min.json
│   ├── ExportDrones_en.json
│   ├── ExportDrones_en.min.json
│   └── ...
├── image/
│   ├── Lotus.Characters.Tenno.Accessory.Scarves.GrnBannerScarf.GrnBannerScarfItem.png
│   ├── Lotus.Characters.Tenno.Accessory.Scarves.PrimeScarfD.Cloth.PrimeScarfDItem.png
│   ├── 256x256/
│   │   ├── Lotus.Characters.Tenno.Accessory.Scarves.GrnBannerScarf.GrnBannerScarfItem.png
│   │   └── ...
│   ├── 128x128/
│   │   └── ...
│   ├── 64x64/
│   │   └── ...
│   └── 32x32/
│       └── ...
├── export_hash.json
└── image_hash.json
```

## Environment Variables

- `OUTPUT_DIRECTORY`: Specify the output directory of the export files (default: `./output`)
- `WARFRAME_ORIGIN_URL`: Specify a custom URL to access warframe origin (default: `https://origin.warframe.com`).
- `PROXY_AUTH_TOKEN`: Specify an authorization token for `WARFRAME_ORIGIN_URL` requests (default: `none`).
