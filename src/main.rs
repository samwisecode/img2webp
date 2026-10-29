use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use image::{DynamicImage, ImageDecoder, ImageReader};
use rayon::prelude::*;
use walkdir::WalkDir;
use webp::{Encoder, WebPConfig};

/// Convert JPEG and PNG images to WebP, aiming for high visual quality at small file sizes.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Input files or directories (directories are searched recursively)
    #[arg(required = true)]
    inputs: Vec<PathBuf>,

    /// Output directory (default: next to each source file)
    #[arg(short, long)]
    out_dir: Option<PathBuf>,

    /// Lossy quality, 0-100. 80-85 is visually near-identical for most photos.
    #[arg(short, long, default_value_t = 82.0, value_parser = parse_quality)]
    quality: f32,

    /// Compression effort, 0 (fast) to 6 (smallest files)
    #[arg(short, long, default_value_t = 6, value_parser = clap::value_parser!(i32).range(0..=6))]
    method: i32,

    /// Always encode lossless (by default PNGs try both lossless and lossy and keep the smaller)
    #[arg(long, conflicts_with = "lossy")]
    lossless: bool,

    /// Always encode lossy, even for PNGs
    #[arg(long)]
    lossy: bool,

    /// Overwrite existing .webp files
    #[arg(short = 'f', long)]
    overwrite: bool,

    /// Don't write the WebP if it would be larger than the source file
    #[arg(long)]
    skip_larger: bool,
}

fn parse_quality(s: &str) -> Result<f32, String> {
    let q: f32 = s.parse().map_err(|_| format!("`{s}` is not a number"))?;
    if (0.0..=100.0).contains(&q) {
        Ok(q)
    } else {
        Err("quality must be between 0 and 100".into())
    }
}

#[derive(Clone, Copy, PartialEq)]
enum SourceKind {
    Jpeg,
    Png,
}

impl SourceKind {
    fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            _ => None,
        }
    }
}

enum Outcome {
    Written {
        src_size: u64,
        out_size: u64,
        out: PathBuf,
        lossless: bool,
    },
    Skipped(String),
}

fn main() -> Result<()> {
    let args = Args::parse();

    let files = collect_files(&args.inputs)?;
    if files.is_empty() {
        bail!("no .jpg, .jpeg or .png files found");
    }
    check_output_collisions(&files, args.out_dir.as_deref())?;
    if let Some(dir) = &args.out_dir {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }

    let results: Vec<_> = files
        .par_iter()
        .map(|(path, root)| {
            let result = convert(path, root, &args);
            match &result {
                Ok(Outcome::Written {
                    src_size,
                    out_size,
                    out,
                    lossless,
                }) => println!(
                    "{} -> {} ({} -> {}, {:+.1}%{})",
                    path.display(),
                    out.display(),
                    human(*src_size),
                    human(*out_size),
                    percent_change(*src_size, *out_size),
                    if *lossless { ", lossless" } else { "" }
                ),
                Ok(Outcome::Skipped(reason)) => println!("{}: skipped ({reason})", path.display()),
                Err(e) => eprintln!("{}: error: {e:#}", path.display()),
            }
            result
        })
        .collect();

    let (mut converted, mut skipped, mut failed) = (0, 0, 0);
    let (mut total_src, mut total_out) = (0u64, 0u64);
    for r in &results {
        match r {
            Ok(Outcome::Written {
                src_size, out_size, ..
            }) => {
                converted += 1;
                total_src += src_size;
                total_out += out_size;
            }
            Ok(Outcome::Skipped(_)) => skipped += 1,
            Err(_) => failed += 1,
        }
    }

    println!("\n{converted} converted, {skipped} skipped, {failed} failed",);
    if converted > 0 {
        println!(
            "Total: {} -> {} ({:+.1}%)",
            human(total_src),
            human(total_out),
            percent_change(total_src, total_out)
        );
    }

    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// Returns (file, root) pairs; `root` is the directory the file was found under,
/// used to mirror the folder structure into `--out-dir`.
fn collect_files(inputs: &[PathBuf]) -> Result<Vec<(PathBuf, PathBuf)>> {
    let mut files = Vec::new();
    for input in inputs {
        if input.is_dir() {
            for entry in WalkDir::new(input) {
                let entry = entry?;
                if entry.file_type().is_file() && SourceKind::from_path(entry.path()).is_some() {
                    files.push((entry.into_path(), input.clone()));
                }
            }
        } else if input.is_file() {
            if SourceKind::from_path(input).is_none() {
                bail!("{}: not a .jpg, .jpeg or .png file", input.display());
            }
            let root = input.parent().map(Path::to_path_buf).unwrap_or_default();
            files.push((input.clone(), root));
        } else {
            bail!("{}: no such file or directory", input.display());
        }
    }
    Ok(files)
}

/// e.g. `photo.jpg` and `photo.png` in the same folder would both become `photo.webp`.
fn check_output_collisions(files: &[(PathBuf, PathBuf)], out_dir: Option<&Path>) -> Result<()> {
    let mut seen = std::collections::HashMap::new();
    for (src, root) in files {
        if let Some(prev) = seen.insert(output_path(src, root, out_dir), src) {
            bail!(
                "{} and {} would both be written to the same .webp file",
                prev.display(),
                src.display()
            );
        }
    }
    Ok(())
}

fn output_path(src: &Path, root: &Path, out_dir: Option<&Path>) -> PathBuf {
    let target = match out_dir {
        Some(dir) => dir.join(
            src.strip_prefix(root)
                .unwrap_or(src.file_name().unwrap().as_ref()),
        ),
        None => src.to_path_buf(),
    };
    target.with_extension("webp")
}

fn convert(src: &Path, root: &Path, args: &Args) -> Result<Outcome> {
    let kind = SourceKind::from_path(src).expect("filtered in collect_files");
    let out = output_path(src, root, args.out_dir.as_deref());
    if out.exists() && !args.overwrite {
        return Ok(Outcome::Skipped(format!(
            "{} exists, use --overwrite",
            out.display()
        )));
    }

    let img = load(src)?;
    let has_alpha = img.color().has_alpha();
    let (pixels, width, height) = if has_alpha {
        let rgba = img.into_rgba8();
        let (w, h) = rgba.dimensions();
        (rgba.into_raw(), w, h)
    } else {
        let rgb = img.into_rgb8();
        let (w, h) = rgb.dimensions();
        (rgb.into_raw(), w, h)
    };
    let encoder = if has_alpha {
        Encoder::from_rgba(&pixels, width, height)
    } else {
        Encoder::from_rgb(&pixels, width, height)
    };

    // JPEGs are already lossy, so lossless WebP would just preserve the JPEG artifacts at
    // a larger size. PNGs are often graphics/screenshots where lossless wins, so try both.
    let try_lossy = !args.lossless;
    let try_lossless = args.lossless || (kind == SourceKind::Png && !args.lossy);

    let mut best: Option<(Vec<u8>, bool)> = None;
    for (enabled, lossless) in [(try_lossy, false), (try_lossless, true)] {
        if !enabled {
            continue;
        }
        let data = encode(&encoder, lossless, args)?;
        if best.as_ref().is_none_or(|(b, _)| data.len() < b.len()) {
            best = Some((data, lossless));
        }
    }
    let (data, lossless) = best.expect("at least one encoding mode is enabled");

    let src_size = fs::metadata(src)?.len();
    let out_size = data.len() as u64;
    if args.skip_larger && out_size >= src_size {
        return Ok(Outcome::Skipped(format!(
            "WebP would be larger ({} vs {})",
            human(out_size),
            human(src_size)
        )));
    }

    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&out, &data).with_context(|| format!("writing {}", out.display()))?;
    Ok(Outcome::Written {
        src_size,
        out_size,
        out,
        lossless,
    })
}

/// Decodes the image and applies its EXIF orientation, since WebP output carries no
/// EXIF and phone photos would otherwise come out rotated.
fn load(path: &Path) -> Result<DynamicImage> {
    let mut decoder = ImageReader::open(path)?
        .with_guessed_format()?
        .into_decoder()
        .context("unsupported or corrupt image")?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).context("decoding image")?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok(img)
}

fn encode(encoder: &Encoder, lossless: bool, args: &Args) -> Result<Vec<u8>> {
    let mut config = WebPConfig::new().map_err(|_| anyhow!("libwebp version mismatch"))?;
    config.method = args.method;
    config.alpha_quality = 100;
    if lossless {
        config.lossless = 1;
        // In lossless mode, quality is compression effort rather than fidelity. Maximum
        // effort is many times slower for ~1-2% smaller files, so cap it (like cwebp -z 6).
        config.quality = 75.0;
        config.method = args.method.min(4);
        config.exact = 0;
    } else {
        config.lossless = 0;
        config.quality = args.quality;
        // Sharper, more accurate RGB->YUV conversion: less colour bleeding on edges.
        config.use_sharp_yuv = 1;
        config.pass = 6;
        config.autofilter = 1;
    }
    let mem = encoder
        .encode_advanced(&config)
        .map_err(|e| anyhow!("WebP encoding failed: {e:?}"))?;
    Ok(mem.to_vec())
}

fn percent_change(from: u64, to: u64) -> f64 {
    if from == 0 {
        return 0.0;
    }
    (to as f64 - from as f64) / from as f64 * 100.0
}

fn human(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}
