# warframe-exports

This project is a Rust-based tool for downloading Warframe public export data, like manifests and images, from the Warframe content servers.

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

- `WARFRAME_ORIGIN_URL`: Specify a custom URL to access warframe origin (default: `https://origin.warframe.com`).
- `PROXY_AUTH_TOKEN`: Specify an authorization token for `WARFRAME_ORIGIN_URL` requests (default: `none`).
