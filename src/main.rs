use anyhow::Context;
use base64::Engine;
use clap::Parser;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use svg::Node;

// ─── Dataset types ───────────────────────────────────────────────────────────

/// A single color category in the dataset.
/// Each category maps a fill color + label to a set of ISO-3166 alpha-3 country codes.
#[derive(Debug, Clone, Deserialize)]
pub struct MapCategory {
    /// CSS hex color, e.g. "#d32f2f"
    pub color: String,
    /// Human-readable label shown in the legend, e.g. "Country at War"
    pub label: String,
    /// ISO 3166-1 alpha-3 codes (matches ISO_A3 / ADM0_A3 in world.json)
    pub country_codes: Vec<String>,
}

/// Top-level dataset that drives the map.
#[derive(Debug, Clone, Deserialize)]
pub struct MapDataset {
    /// Title rendered in the title bar
    pub title: String,
    /// Color categories — order matters: first listed = first in legend
    pub categories: Vec<MapCategory>,
    /// Output file name (without extension). Defaults to "world_map"
    #[serde(default = "default_output")]
    pub output: String,
    /// Country codes to erase from the map (shown as ocean)
    #[serde(default)]
    pub erased_countries: Vec<String>,
}

fn default_output() -> String { "world_map".into() }

impl MapDataset {
    /// Build a lookup: country_code → (fill_color, stroke_color)
    fn build_color_map(&self) -> HashMap<String, (String, String)> {
        let mut map = HashMap::new();
        for cat in &self.categories {
            let stroke = darken_hex(&cat.color, 0.7);
            for code in &cat.country_codes {
                map.insert(code.clone(), (cat.color.clone(), stroke.clone()));
            }
        }
        map
    }
}

/// Scale each RGB channel of a hex color by `factor` (0..1) to produce a darker shade.
fn darken_hex(hex: &str, factor: f64) -> String {
    let hex = hex.trim_start_matches('#');
    if hex.len() < 6 { return "#000000".into(); }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    format!("#{:02x}{:02x}{:02x}",
        (r as f64 * factor) as u8,
        (g as f64 * factor) as u8,
        (b as f64 * factor) as u8,
    )
}

/// Extract the ISO alpha-3 country code from a GeoJSON feature's properties.
/// Falls back from `ISO_A3` to `ADM0_A3` for entities like Palestine, Kosovo, Somaliland.
fn get_country_code(properties: &serde_json::Value) -> &str {
    properties["ISO_A3"]
        .as_str()
        .filter(|s| *s != "-99")
        .or_else(|| properties["ADM0_A3"].as_str())
        .unwrap_or("???")
}

/// Style parameters for polygon rendering — groups fill/stroke args to reduce argument count.
struct PolygonStyle<'a> {
    fill: &'a str,
    stroke: &'a str,
    stroke_width: f64,
    fill_opacity: f64,
}

// ─── Country base color palette ──────────────────────────────────────────────

/// Muted earthy tones for country base fills — visually distinct but not distracting.
const BASE_PALETTE: &[&str] = &[
    "#d6d0b8", // warm sand
    "#c8d4b8", // sage green
    "#dcd2c0", // parchment
    "#c4ccb8", // dusty olive
    "#d8cdb4", // wheat
    "#ccd0c0", // grey-green
    "#ddd4bc", // pale gold
    "#c0c8b4", // muted moss
    "#d4ccb0", // tan
    "#c8d0bc", // soft fern
];

/// Simple hash to pick a palette index from a country code.
fn country_base_color(code: &str) -> &'static str {
    let hash = code.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    BASE_PALETTE[(hash as usize) % BASE_PALETTE.len()]
}

// ─── CLI ─────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "llm-insanity", about = "Generate a colored world map PNG from a JSON dataset")]
struct Cli {
    /// Path to dataset JSON file
    #[arg(short, long)]
    dataset: String,
}

// Embedded data at compile time
fn get_world_geojson() -> &'static str {
    include_str!("../processed_data/world.json")
}

fn get_glaciated_geojson() -> &'static str {
    include_str!("../processed_data/glaciated_areas.json")
}

fn get_lakes_geojson() -> &'static str {
    include_str!("../processed_data/lakes.json")
}

fn get_rivers_geojson() -> &'static str {
    include_str!("../processed_data/rivers.json")
}


fn get_shaded_relief_png() -> &'static [u8] {
    include_bytes!("../processed_data/shaded_relief.png")
}

fn get_ocean_relief_png() -> &'static [u8] {
    include_bytes!("../processed_data/ocean_relief.png")
}

/// Maximum Mercator Y-value at ±80° latitude (computed once).
static MAX_MERC: std::sync::LazyLock<f64> = std::sync::LazyLock::new(|| {
    (std::f64::consts::PI / 4.0 + 80.0_f64.to_radians() / 2.0)
        .tan()
        .ln()
});

fn project_mercator(lon: f64, lat: f64, width: f64, height: f64) -> (f64, f64) {
    let x = ((lon + 180.0) / 360.0) * width;
    let lat_rad = lat.clamp(-80.0, 80.0).to_radians();
    let merc = (std::f64::consts::PI / 4.0 + lat_rad / 2.0).tan().ln();
    let y = (height / 2.0) * (1.0 - merc / *MAX_MERC);
    (x, y)
}

/// Append projected coordinates as SVG path M/L commands.
fn build_ring_path(path_data: &mut String, ring: &[[f64; 2]], width: f64, height: f64) {
    for (i, &[lon, lat]) in ring.iter().enumerate() {
        let (x, y) = project_mercator(lon, lat, width, height);
        if i == 0 {
            let _ = write!(path_data, "M {} {}", x, y);
        } else {
            let _ = write!(path_data, " L {} {}", x, y);
        }
    }
}

fn draw_polygon(
    parent: &mut svg::node::element::Group,
    coords: &[Vec<[f64; 2]>],
    width: f64,
    height: f64,
    style: &PolygonStyle,
) {
    for ring in coords {
        let mut path_data = String::with_capacity(ring.len() * 24);
        build_ring_path(&mut path_data, ring, width, height);
        path_data.push_str(" Z");
        let path = svg::node::element::Path::new()
            .set("d", path_data)
            .set("fill", style.fill)
            .set("stroke", style.stroke)
            .set("stroke-width", style.stroke_width)
            .set("fill-opacity", style.fill_opacity);
        parent.append(path);
    }
}

/// Draw a line (for rivers, etc.) — no fill, just stroke.
fn draw_line(
    parent: &mut svg::node::element::Group,
    coords: &[[f64; 2]],
    width: f64,
    height: f64,
    stroke: &str,
    stroke_width: f64,
    opacity: f64,
) {
    let mut path_data = String::with_capacity(coords.len() * 24);
    build_ring_path(&mut path_data, coords, width, height);
    let path = svg::node::element::Path::new()
        .set("d", path_data)
        .set("fill", "none")
        .set("stroke", stroke)
        .set("stroke-width", stroke_width)
        .set("opacity", opacity)
        .set("stroke-linecap", "round")
        .set("stroke-linejoin", "round");
    parent.append(path);
}

fn extract_ring_coords(ring: &serde_json::Value) -> Vec<[f64; 2]> {
    ring.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|point| {
                    point.as_array().and_then(|p| {
                        if p.len() >= 2 {
                            Some([p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0)])
                        } else {
                            None
                        }
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extract_polygon_coords(coords: &serde_json::Value) -> Vec<Vec<[f64; 2]>> {
    coords
        .as_array()
        .map(|rings| rings.iter().map(extract_ring_coords).collect())
        .unwrap_or_default()
}

/// Extract all polygon coordinate rings from a geometry (Polygon or MultiPolygon).
fn extract_all_polygon_coords(geometry: &serde_json::Value) -> Vec<Vec<Vec<[f64; 2]>>> {
    match geometry["type"].as_str().unwrap_or("") {
        "Polygon" => vec![extract_polygon_coords(&geometry["coordinates"])],
        "MultiPolygon" => geometry["coordinates"]
            .as_array()
            .map(|polygons| polygons.iter().map(extract_polygon_coords).collect())
            .unwrap_or_default(),
        _ => vec![],
    }
}

/// Render all polygon/multipolygon features from a GeoJSON FeatureCollection.
fn render_geojson_polygons(
    geojson: &serde_json::Value,
    group: &mut svg::node::element::Group,
    width: f64,
    height: f64,
    style: &PolygonStyle,
) {
    let Some(features) = geojson["features"].as_array() else { return };
    for feature in features {
        let geometry = &feature["geometry"];
        if geometry.is_null() { continue; }
        for coords in extract_all_polygon_coords(geometry) {
            if !coords.is_empty() {
                draw_polygon(group, &coords, width, height, style);
            }
        }
    }
}

/// Render all line features from a GeoJSON FeatureCollection (LineString / MultiLineString).
fn render_geojson_lines(
    geojson: &serde_json::Value,
    group: &mut svg::node::element::Group,
    width: f64,
    height: f64,
    stroke: &str,
    stroke_width: f64,
    opacity: f64,
) {
    let Some(features) = geojson["features"].as_array() else { return };
    for feature in features {
        let geometry = &feature["geometry"];
        if geometry.is_null() { continue; }
        let geom_type = geometry["type"].as_str().unwrap_or("");
        match geom_type {
            "LineString" => {
                let coords = extract_ring_coords(&geometry["coordinates"]);
                if !coords.is_empty() {
                    draw_line(group, &coords, width, height, stroke, stroke_width, opacity);
                }
            }
            "MultiLineString" => {
                if let Some(lines) = geometry["coordinates"].as_array() {
                    for line in lines {
                        let coords = extract_ring_coords(line);
                        if !coords.is_empty() {
                            draw_line(group, &coords, width, height, stroke, stroke_width, opacity);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Iterate non-erased countries, calling `style_fn(country_code)` to determine the polygon
/// style. Return `None` from the closure to skip a country.
fn render_countries<'a>(
    world: &serde_json::Value,
    erased: &HashSet<&str>,
    group: &mut svg::node::element::Group,
    width: f64,
    height: f64,
    style_fn: impl Fn(&str) -> Option<PolygonStyle<'a>>,
) {
    let Some(features) = world["features"].as_array() else { return };
    for feature in features {
        let code = get_country_code(&feature["properties"]);
        if erased.contains(code) { continue; }
        let Some(style) = style_fn(code) else { continue };
        let geometry = &feature["geometry"];
        if geometry.is_null() { continue; }
        for coords in extract_all_polygon_coords(geometry) {
            if !coords.is_empty() {
                draw_polygon(group, &coords, width, height, &style);
            }
        }
    }
}

fn generate_world_map(dataset: &MapDataset, output_name: &str) -> anyhow::Result<()> {
    let width = 3840.0;
    let height = 1920.0;

    // Build country_code → (fill, stroke) lookup from dataset
    let color_map = dataset.build_color_map();
    let erased: HashSet<&str> = dataset.erased_countries.iter().map(|s| s.as_str()).collect();

    let mut document = svg::Document::new()
        .set("width", width)
        .set("height", height)
        .set("viewBox", (0, 0, width as i32, height as i32));

    // ── Ocean background ──
    document.append(
        svg::node::element::Rectangle::new()
            .set("width", "100%")
            .set("height", "100%")
            .set("fill", "#1a5276"),
    );

    // ── Ocean bathymetry relief ──
    {
        let ocean_b64 = base64::engine::general_purpose::STANDARD.encode(get_ocean_relief_png());
        let data_uri = format!("data:image/png;base64,{ocean_b64}");
        document.append(
            svg::node::element::Image::new()
                .set("x", 0)
                .set("y", 0)
                .set("width", width)
                .set("height", height)
                .set("href", data_uri.as_str())
                .set("preserveAspectRatio", "none"),
        );
    }

    // ── Graticule grid ──
    document.append(build_graticule(width, height));

    // ── Parse all GeoJSON data ──
    let world: serde_json::Value =
        serde_json::from_str(get_world_geojson()).context("Failed to parse world.json")?;
    let glaciated: serde_json::Value =
        serde_json::from_str(get_glaciated_geojson()).context("Failed to parse glaciated_areas.json")?;
    let lakes: serde_json::Value =
        serde_json::from_str(get_lakes_geojson()).context("Failed to parse lakes.json")?;
    let rivers: serde_json::Value =
        serde_json::from_str(get_rivers_geojson()).context("Failed to parse rivers.json")?;

    // ── Layer 1: Country base shapes (varied earthy fills) ──
    let mut base_group = svg::node::element::Group::new().set("id", "countries-base");
    render_countries(&world, &erased, &mut base_group, width, height, |code| {
        Some(PolygonStyle {
            fill: country_base_color(code),
            stroke: "#a09880",
            stroke_width: 0.3,
            fill_opacity: 0.90,
        })
    });
    document.append(base_group);

    // ── Land-mask clip-path ──
    document.append(build_land_mask(&world, &erased, width, height));

    // ── Layer 3: Glaciated / ice areas ──
    let mut ice_group = svg::node::element::Group::new().set("id", "glaciated");
    render_geojson_polygons(
        &glaciated, &mut ice_group, width, height,
        &PolygonStyle { fill: "#f0f5f8", stroke: "none", stroke_width: 0.0, fill_opacity: 0.85 },
    );
    document.append(ice_group);

    // ── Layer 4: Lakes ──
    let mut lakes_group = svg::node::element::Group::new().set("id", "lakes");
    render_geojson_polygons(
        &lakes, &mut lakes_group, width, height,
        &PolygonStyle { fill: "#3a80b8", stroke: "#2a5a88", stroke_width: 0.4, fill_opacity: 0.92 },
    );
    document.append(lakes_group);

    // ── Layer 5: Rivers ──
    let mut rivers_group = svg::node::element::Group::new().set("id", "rivers");
    render_geojson_lines(&rivers, &mut rivers_group, width, height, "#4a90c8", 0.9, 0.75);
    document.append(rivers_group);

    // ── Layer 6: Dataset color overlay (semi-transparent) ──
    let mut overlay_group = svg::node::element::Group::new().set("id", "data-overlay");
    render_countries(&world, &erased, &mut overlay_group, width, height, |code| {
        let (fill, _) = color_map.get(code)?;
        Some(PolygonStyle {
            fill: fill.as_str(),
            stroke: "none",
            stroke_width: 0.0,
            fill_opacity: 0.62,
        })
    });
    document.append(overlay_group);

    // ── Shaded relief overlay (RGBA highlight/shadow, clipped to land) ──
    {
        let relief_b64 = base64::engine::general_purpose::STANDARD.encode(get_shaded_relief_png());
        let data_uri = format!("data:image/png;base64,{relief_b64}");
        document.append(
            svg::node::element::Image::new()
                .set("x", 0)
                .set("y", 0)
                .set("width", width)
                .set("height", height)
                .set("href", data_uri.as_str())
                .set("clip-path", "url(#land-mask)")
                .set("preserveAspectRatio", "none"),
        );
    }

    // ── Layer 7: Country borders (crisp lines on top) ──
    let mut borders_group = svg::node::element::Group::new().set("id", "borders");
    render_countries(&world, &erased, &mut borders_group, width, height, |_| {
        Some(PolygonStyle {
            fill: "none",
            stroke: "#505050",
            stroke_width: 0.5,
            fill_opacity: 0.0,
        })
    });
    document.append(borders_group);

    // ── Legend & Title ──
    document.append(build_legend(dataset));
    document.append(build_title_bar(&dataset.title, width));

    // ── Save SVG ──
    std::fs::create_dir_all("generated")?;
    let svg_path = format!("generated/{output_name}_4k.svg");
    svg::save(&svg_path, &document)?;
    println!("✓ SVG saved to {svg_path}");

    // ── Convert to PNG (4K + HD) ──
    let png_4k_path = format!("generated/{output_name}_4k.png");
    let png_hd_path = format!("generated/{output_name}_hd.png");
    convert_svg_to_png(&svg_path, &png_4k_path, &png_hd_path)?;

    // ── Summary ──
    for cat in &dataset.categories {
        println!("  {} — {} countries ({})", cat.label, cat.country_codes.len(), cat.color);
    }

    Ok(())
}

// ─── Sub-builders ────────────────────────────────────────────────────────────

fn build_graticule(width: f64, height: f64) -> svg::node::element::Group {
    let mut group = svg::node::element::Group::new().set("id", "graticule");
    for lon in (-180..180).step_by(30) {
        let mut d = String::with_capacity(512);
        for lat in -80..=80 {
            let (x, y) = project_mercator(lon as f64, lat as f64, width, height);
            if lat == -80 {
                let _ = write!(d, "M {} {}", x, y);
            } else {
                let _ = write!(d, " L {} {}", x, y);
            }
        }
        group.append(
            svg::node::element::Path::new()
                .set("d", d)
                .set("fill", "none")
                .set("stroke", "#2a6090")
                .set("stroke-width", 0.3)
                .set("opacity", 0.35),
        );
    }
    for lat in (-80..=80).step_by(20) {
        let mut d = String::with_capacity(1024);
        for lon in -180..=180 {
            let (x, y) = project_mercator(lon as f64, lat as f64, width, height);
            if lon == -180 {
                let _ = write!(d, "M {} {}", x, y);
            } else {
                let _ = write!(d, " L {} {}", x, y);
            }
        }
        group.append(
            svg::node::element::Path::new()
                .set("d", d)
                .set("fill", "none")
                .set("stroke", "#2a6090")
                .set("stroke-width", 0.3)
                .set("opacity", 0.35),
        );
    }
    group
}

fn build_land_mask(
    world: &serde_json::Value,
    erased: &HashSet<&str>,
    width: f64,
    height: f64,
) -> svg::node::element::Definitions {
    let mut land_path_data = String::new();
    if let Some(features) = world["features"].as_array() {
        for feature in features {
            let code = get_country_code(&feature["properties"]);
            if erased.contains(code) { continue; }
            let geometry = &feature["geometry"];
            if geometry.is_null() { continue; }
            for coords in extract_all_polygon_coords(geometry) {
                for ring in &coords {
                    for (i, &[lon, lat]) in ring.iter().enumerate() {
                        let (x, y) = project_mercator(lon, lat, width, height);
                        if i == 0 {
                            let _ = write!(land_path_data, "M {} {} ", x, y);
                        } else {
                            let _ = write!(land_path_data, "L {} {} ", x, y);
                        }
                    }
                    land_path_data.push_str("Z ");
                }
            }
        }
    }
    let land_path = svg::node::element::Path::new().set("d", land_path_data);
    let clip = svg::node::element::ClipPath::new()
        .set("id", "land-mask")
        .add(land_path);
    svg::node::element::Definitions::new().add(clip)
}

fn build_legend(dataset: &MapDataset) -> svg::node::element::Group {
    let cat_count = dataset.categories.len();
    let row_h = 44;
    let top_pad = 56;
    let bot_pad = 24;
    let legend_w = 420;
    let legend_h = top_pad + (cat_count as i32) * row_h + bot_pad;

    let mut group = svg::node::element::Group::new()
        .set("id", "legend")
        .set("transform", "translate(40, 120)");

    // Shadow
    group.append(
        svg::node::element::Rectangle::new()
            .set("x", 4).set("y", 4)
            .set("width", legend_w).set("height", legend_h)
            .set("rx", 16).set("ry", 16)
            .set("fill", "#000000").set("opacity", 0.25),
    );

    // Background panel
    group.append(
        svg::node::element::Rectangle::new()
            .set("width", legend_w).set("height", legend_h)
            .set("rx", 16).set("ry", 16)
            .set("fill", "#1a2332")
            .set("stroke", "rgba(255,255,255,0.08)")
            .set("stroke-width", 1).set("opacity", 0.88),
    );

    // Header
    group.append(
        svg::node::element::Text::new()
            .set("x", legend_w / 2).set("y", 38)
            .set("font-size", "20").set("font-weight", "600")
            .set("text-anchor", "middle").set("font-family", "sans-serif")
            .set("fill", "#e0e6ed").set("letter-spacing", "1.5")
            .add(svg::node::Text::new("LEGEND")),
    );

    // Category rows
    let mut ly = top_pad;
    for cat in &dataset.categories {
        group.append(
            svg::node::element::Rectangle::new()
                .set("x", 24).set("y", ly + 2)
                .set("width", 32).set("height", 24)
                .set("fill", cat.color.as_str())
                .set("stroke", "rgba(255,255,255,0.15)").set("stroke-width", 1)
                .set("fill-opacity", 0.85)
                .set("rx", 6).set("ry", 6),
        );
        group.append(
            svg::node::element::Text::new()
                .set("x", 68).set("y", ly + 20)
                .set("font-size", "17").set("font-family", "sans-serif")
                .set("font-weight", "500").set("fill", "#e8ecf0")
                .add(svg::node::Text::new(&cat.label)),
        );
        ly += row_h;
    }

    group
}

fn build_title_bar(title: &str, width: f64) -> svg::node::element::Group {
    let title_w = 1400.0_f64.min(width * 0.6);
    let title_h = 72.0;
    let title_x = (width - title_w) / 2.0;
    let title_y = 28.0;

    let mut group = svg::node::element::Group::new().set("id", "title");

    // Shadow
    group.append(
        svg::node::element::Rectangle::new()
            .set("x", title_x + 3.0).set("y", title_y + 3.0)
            .set("width", title_w).set("height", title_h)
            .set("rx", 20).set("ry", 20)
            .set("fill", "#000000").set("opacity", 0.3),
    );

    // Background pill
    group.append(
        svg::node::element::Rectangle::new()
            .set("x", title_x).set("y", title_y)
            .set("width", title_w).set("height", title_h)
            .set("rx", 20).set("ry", 20)
            .set("fill", "#0d1b2a")
            .set("stroke", "rgba(255,255,255,0.10)")
            .set("stroke-width", 1).set("opacity", 0.90),
    );

    // Title text
    group.append(
        svg::node::element::Text::new()
            .set("x", width / 2.0).set("y", title_y + 48.0)
            .set("font-size", "30").set("font-weight", "700")
            .set("text-anchor", "middle").set("font-family", "sans-serif")
            .set("fill", "#ffffff").set("letter-spacing", "0.5")
            .add(svg::node::Text::new(title)),
    );

    group
}

// ─── PNG conversion ──────────────────────────────────────────────────────────

/// Write RGBA pixel data to a PNG file with maximum lossless compression.
fn save_optimized_png(path: &str, width: u32, height: u32, data: &[u8]) -> anyhow::Result<()> {
    let file = std::fs::File::create(path).context("Failed to create PNG file")?;
    let buf = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(buf, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Best);
    encoder.set_adaptive_filter(png::AdaptiveFilterType::Adaptive);
    let mut writer = encoder.write_header().context("Failed to write PNG header")?;
    writer.write_image_data(data).context("Failed to write PNG data")?;
    drop(writer);

    let size_kb = std::fs::metadata(path)?.len() / 1024;
    println!("✓ PNG saved to {path} ({size_kb} KB)");
    Ok(())
}

fn convert_svg_to_png(svg_path: &str, png_path: &str, png_1080p_path: &str) -> anyhow::Result<()> {
    use resvg::usvg;
    use std::sync::Arc;

    let svg_data = std::fs::read(svg_path).context("Failed to read SVG file")?;

    let mut fontdb = usvg::fontdb::Database::new();
    fontdb.load_system_fonts();

    let options = usvg::Options {
        fontdb: Arc::new(fontdb),
        font_family: "sans-serif".into(),
        ..Default::default()
    };

    let tree = usvg::Tree::from_data(&svg_data, &options).context("Failed to parse SVG")?;
    let size = tree.size();
    let full_w = size.width().ceil() as u32;
    let full_h = size.height().ceil() as u32;

    // ── Full-resolution render ──
    let mut pixmap = tiny_skia::Pixmap::new(full_w, full_h)
        .ok_or_else(|| anyhow::anyhow!("Failed to create {full_w}×{full_h} pixmap"))?;
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    save_optimized_png(png_path, full_w, full_h, pixmap.data())?;

    // ── 1080p render (vector-quality downscale via resvg) ──
    let target_h = 1080u32;
    let scale = target_h as f32 / full_h as f32;
    let target_w = (full_w as f32 * scale).ceil() as u32;
    let mut pixmap_1080 = tiny_skia::Pixmap::new(target_w, target_h)
        .ok_or_else(|| anyhow::anyhow!("Failed to create {target_w}×{target_h} pixmap"))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap_1080.as_mut(),
    );
    save_optimized_png(png_1080p_path, target_w, target_h, pixmap_1080.data())?;

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let dataset_path = std::path::Path::new(&cli.dataset);
    let output_name = dataset_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("world_map");
    let json_str = std::fs::read_to_string(dataset_path)
        .with_context(|| format!("Failed to read dataset '{}'", cli.dataset))?;
    let dataset: MapDataset = serde_json::from_str(&json_str)
        .context("Failed to parse dataset JSON")?;
    generate_world_map(&dataset, output_name)?;
    Ok(())
}
