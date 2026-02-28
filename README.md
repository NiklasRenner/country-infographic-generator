# country-infographic-generator

A Rust CLI tool that generates world map PNGs with countries colored by a user-provided JSON dataset. Features shaded relief terrain, ocean bathymetry, natural features (glaciers, lakes, rivers), and a clean data-driven legend.

## Quick Start

```sh
cargo build --release -q
cargo run --release -q -- --dataset datasets/conflicts_feb2026.json
```

Or use the shortcut:k

```sh
.\run.ps1
```

## Output

Generates two files in `generated/`:

- `generated/<output>.svg` — vector map (3840×1920)
- `generated/<output>.png` — rasterized bitmap

## Dataset Format

Datasets live in `datasets/` as JSON files:

```json
{
  "title": "World Map — Active Conflicts (February 2026)",
  "output": "world_map_conflicts",
  "erased_countries": ["SWE"],
  "categories": [
    {
      "color": "#d32f2f",
      "label": "Country at War",
      "country_codes": ["UKR", "RUS", "PSE", "SAU"]
    }
  ]
}
```

### Fields

| Field | Required | Description |
|---|---|---|
| `title` | yes | Title text rendered on the map |
| `categories` | yes | Array of color categories (see below) |
| `output` | no | Output filename without extension (default: `world_map`) |
| `erased_countries` | no | Array of ISO alpha-3 codes to erase (shown as ocean) |

### Category Fields

| Field | Description |
|---|---|
| `color` | CSS hex color, e.g. `#d32f2f` |
| `label` | Legend text — avoid `<` and `>` characters (breaks SVG/XML) |
| `country_codes` | Array of ISO 3166-1 alpha-3 codes |

Country codes match `ISO_A3` or `ADM0_A3` in the GeoJSON data (Natural Earth 50m).

## Example Datasets

| File | Description |
|---|---|
| `conflicts_feb2026.json` | Countries with active armed conflicts |
| `political_systems.json` | Democracy index classification |
| `murder_rate.json` | Intentional homicide rate per 100k |
| `gay_countries.json` | Just Bulgaria |

## Map Layers

The map is composed of these layers (bottom to top):

1. **Ocean** — solid background + bathymetry relief image
2. **Graticule** — 30° longitude / 20° latitude grid lines
3. **Country base** — neutral tan fill for all land
4. **Land mask** — clip-path used by relief overlay
5. **Glaciated areas** — ice/glacier polygons
6. **Lakes** — major lake polygons
7. **Rivers** — major river lines
8. **Data overlay** — semi-transparent country fills from dataset
9. **Shaded relief** — terrain highlight/shadow (clipped to land)
10. **Borders** — country border lines
11. **Legend** — floating panel with dataset categories
12. **Title** — floating pill-shaped title bar

## Architecture

Single-file Rust project (`src/main.rs`). All geographic data is embedded at compile time via `include_str!` / `include_bytes!`:

- `processed_data/world.json` — Natural Earth 50m country boundaries (GeoJSON)
- `processed_data/glaciated_areas.json` — glacier/ice polygons
- `processed_data/lakes.json` — major lakes
- `processed_data/rivers.json` — major rivers
- `processed_data/shaded_relief.png` — land terrain shading
- `processed_data/ocean_relief.png` — ocean bathymetry

Projection: Mercator, clamped to ±80° latitude.

## Dependencies

- `svg` — SVG construction
- `resvg` + `tiny-skia` — SVG → PNG rasterization (pure Rust, no external tools)
- `serde` + `serde_json` — JSON parsing
- `clap` — CLI argument parsing
- `anyhow` — error handling
- `base64` — encoding relief PNGs as data URIs
