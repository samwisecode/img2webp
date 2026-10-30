# img2webp

A fast command-line tool that converts JPEG and PNG images to WebP, tuned for high visual quality at small file sizes.

- Converts single files or whole directory trees (searched recursively)
- Encodes files in parallel across all CPU cores
- Chooses lossy or lossless per file: JPEGs are encoded lossy, while PNGs are encoded both ways and the smaller result is kept
- Applies EXIF orientation, so phone photos don't come out rotated
- Prints per-file and total size savings

## Installation

### Prebuilt binaries

macOS and Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/samwisecode/img2webp/releases/latest/download/img2webp-installer.sh | sh
```

Windows (PowerShell):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/samwisecode/img2webp/releases/latest/download/img2webp-installer.ps1 | iex"
```

Archives for each platform are also attached to every [GitHub release](https://github.com/samwisecode/img2webp/releases).

### From source

Requires Rust 1.85 or newer (edition 2024) and a C compiler, since libwebp is built from source as part of the build.

```sh
# Install from a local checkout
cargo install --path .

# Or build without installing
cargo build --release
# Binary is at target/release/img2webp
```

## Usage

```sh
img2webp [OPTIONS] <INPUTS>...
```

`INPUTS` can be any mix of `.jpg`, `.jpeg` and `.png` files and directories. Extensions are matched case-insensitively.

### Examples

```sh
# Convert one image; writes photo.webp next to photo.jpg
img2webp photo.jpg

# Convert everything under ./images, writing each .webp next to its source
img2webp images/

# Write output to a separate folder, mirroring the input folder structure
img2webp images/ --out-dir dist/images

# Smaller files at slightly lower quality, and skip any that would grow
img2webp images/ -q 75 --skip-larger

# Faster encoding with less compression effort
img2webp images/ -m 4

# Re-run and replace existing .webp files
img2webp images/ --overwrite
```

### Options

| Option | Default | Description |
| --- | --- | --- |
| `-o, --out-dir <DIR>` | next to source | Output directory. Subfolder structure of directory inputs is mirrored into it. |
| `-q, --quality <0-100>` | `82` | Lossy quality. 80–85 is visually near-identical for most photos. |
| `-m, --method <0-6>` | `6` | Compression effort, from 0 (fastest) to 6 (smallest files). |
| `--lossless` | off | Always encode lossless. |
| `--lossy` | off | Always encode lossy, even for PNGs. Conflicts with `--lossless`. |
| `-f, --overwrite` | off | Overwrite existing `.webp` files. Without this, existing outputs are skipped. |
| `--skip-larger` | off | Don't write the WebP if it would be the same size or larger than the source. |
| `-h, --help` | | Print help. |
| `-V, --version` | | Print version. |

### Output

```
images/hero.jpg -> images/hero.webp (1.2 MB -> 214.6 KB, -82.5%)
images/logo.png -> images/logo.webp (48.3 KB -> 12.1 KB, -74.9%, lossless)
images/icon.png: skipped (images/icon.webp exists, use --overwrite)

2 converted, 1 skipped, 0 failed
Total: 1.2 MB -> 226.7 KB (-82.2%)
```

Errors for individual files are printed to stderr and don't stop the run. The process exits with status `1` if any file failed.

## How it works

**Lossy vs. lossless.** JPEGs are already lossy, so encoding them losslessly would only preserve the JPEG artifacts at a larger size; they are always encoded lossy (unless `--lossless` is given). PNGs are often screenshots or graphics where lossless compression wins, so by default both encodings are tried and the smaller one is written.

**Lossy settings.** Lossy encoding uses sharp RGB→YUV conversion (less colour bleeding at edges), 6 entropy passes, automatic filter strength and full alpha quality.

**Lossless settings.** In lossless mode, libwebp's quality setting controls compression effort rather than fidelity. The tool uses quality 75 and caps the method at 4: maximum effort is many times slower for only about 1–2% smaller files.

**Safety checks.** Before converting anything, the tool checks that no two inputs map to the same output file (for example `photo.jpg` and `photo.png` in one folder) and stops with an error if they do.

## Limitations

- Metadata (EXIF, ICC colour profiles) is not copied to the WebP. EXIF orientation is applied to the pixels before encoding.
- Animated PNGs are converted as a single still frame.
- 16-bit PNGs are reduced to 8 bits per channel, since WebP only supports 8-bit.

## License

[MIT](LICENSE)
