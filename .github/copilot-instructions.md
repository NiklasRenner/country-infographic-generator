# Copilot Instructions — country-infographic-generator

![example.png](../example.png)

## Project Overview

A Rust CLI tool that generates a world map (SVG → PNG) with countries colored according to a user-provided JSON dataset. It reads GeoJSON country boundaries from `world.json`, projects them with Mercator, renders styled SVG with a data-driven legend, and converts the result to PNG using `resvg`/`tiny-skia`.

## Tech Stack

- **Language:** Rust (edition 2024)
- **Build:** `cargo` — always use `-q` (quiet) flag for build/run commands to avoid noisy output
- **Key crates:**
  - `svg` — programmatic SVG construction
  - `resvg` + `tiny-skia` — SVG-to-PNG rasterization (no external tools, no Python)
  - `serde` / `serde_json` — JSON parsing for GeoJSON and dataset data
  - `clap` — CLI argument parsing (`--dataset <path>`)
  - `anyhow` — error handling
  - `base64` — encoding relief PNGs as data URIs in SVG

## Architecture

Single-file project (`src/main.rs`). Key structure:

1. **Dataset input** — `MapDataset` struct loaded from a JSON file at runtime via `--dataset` CLI arg. Contains:
   - `title` — rendered in the title bar
   - `categories` — array of `MapCategory`, each with `color` (hex), `label`, and `country_codes` (ISO alpha-3)
   - `output` — base filename (without extension) for output files
   - `erased_countries` — ISO alpha-3 codes to erase from the map (rendered as ocean)
2. **GeoJSON data** — all embedded at compile time via `include_str!`:
   - `processed_data/world.json` — country polygons (Natural Earth 50m, `ISO_A3` / `ADM0_A3` for matching)
   - `processed_data/glaciated_areas.json` — ice/glacier polygons (Natural Earth 110m)
   - `processed_data/lakes.json` — major lake polygons (Natural Earth 110m)
   - `processed_data/rivers.json` — major river lines (Natural Earth 110m)
3. **Relief imagery** — embedded at compile time via `include_bytes!`:
   - `processed_data/shaded_relief.png` — terrain highlight/shadow (clipped to land via land-mask)
   - `processed_data/ocean_relief.png` — ocean bathymetry
4. **Projection** — `project_mercator()` maps lon/lat → pixel coords, clamped to ±80° latitude.
5. **SVG generation** — builds layers in order: ocean background + bathymetry → graticule grid → country base shapes → land-mask clip-path → glaciated areas → lakes → rivers → dataset color overlay → shaded relief → country borders → legend → title bar.
6. **Reusable renderers** — `render_geojson_polygons()` and `render_geojson_lines()` handle any GeoJSON FeatureCollection for polygon/line types.
7. **PNG conversion** — `convert_svg_to_png()` loads system fonts, parses SVG with `usvg`, renders with `resvg`. Text rendering requires system fonts to be available.

## Dataset JSON Format

```json
{
  "title": "World Map — Active Conflicts (February 2026)",
  "output": "world_map_conflicts",
  "erased_countries": [],
  "categories": [
    {
      "color": "#d32f2f",
      "label": "Country at War",
      "country_codes": ["UKR", "RUS", "PSE", ...]
    }
  ]
}
```

- **`title`** — rendered in the floating title bar
- **`color`** — CSS hex color for country fill
- **`label`** — legend text (avoid `<` and `>` — breaks SVG/XML parsing)
- **`country_codes`** — ISO 3166-1 alpha-3 codes matching `ISO_A3` / `ADM0_A3` in `world.json`
- **`erased_countries`** — optional array of country codes to remove from the map entirely (shown as ocean)
- Multiple categories are supported; each gets its own legend entry and color
- The legend only shows dataset categories
- Example datasets in `datasets/`

## Output Files

- `generated/<output>.svg` — vector map (3840×1920)
- `generated/<output>.png` — rasterized bitmap with text

Both are generated in the `generated/` directory.

## Conventions

- Use `anyhow::Result` for all fallible functions.
- Country matching uses `ISO_A3` (falling back to `ADM0_A3` for entities like Palestine, Kosovo, Somaliland). Verify exact codes in `world.json`.
- Colors: ocean = `#1a5276`, country base = `#e0dbc8`, ice = `#f0f5f8`, lakes = `#3a80b8`, rivers = `#4a90c8`, borders = `#505050`. Country colors come from the dataset.
- Font family for all SVG text: `sans-serif`.
- Erased countries are skipped in all rendering layers (base, land-mask, overlay, borders).

## Common Commands

```sh
cargo build --release -q                                              # Build (quiet)
cargo run --release -q -- --dataset datasets/conflicts_feb2026.json   # Run with dataset
.\run.ps1                                                             # Shortcut script
```

## Important Notes

- Do NOT use Python or external tools for image conversion — everything is pure Rust.
- The `processed_data/world.json` file is large (~1.8MB, Natural Earth 50m). Don't try to read/print it entirely; use targeted searches.
- The legend is fully data-driven: category rows come from the dataset, followed by the default color. No static base-map entries.

