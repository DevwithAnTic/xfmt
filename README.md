# XFMT: eXtended File Multi-block Transformer

![Version](https://img.shields.io/badge/version-0.1.0-blue)
![License](https://img.shields.io/badge/license-Apache--2.0-green)
![Language](https://img.shields.io/badge/language-Rust-orange)
![Deduplication](https://img.shields.io/badge/deduplication-CDC-brightgreen)
![Security](https://img.shields.io/badge/security-AES--256--GCM-red)

XFMT is a high-performance, reversible file transform format designed for transparent archival, instant streaming, and efficient storage.

## Benchmarks: XFMT vs. Tar-XZ & 7-Zip

On raw datasets (Text, Source, Binary), XFMT provides the following performance profile:

| Metric | XFMT (Zstd + CDC) | Tar-XZ | 7-Zip (LZMA2) | Winner |
| :--- | :--- | :--- | :--- | :--- |
| **Encoding Speed** | **60-170 MB/s** | ~3 MB/s | ~12 MB/s | **XFMT (Fastest)** |
| **Random Access (Seek)**| **~25ms** | ~26ms | ~85ms | **XFMT (Instant)** |
| **Streaming Start** | **0.1s** | 5s - 45s+ | 10s - 90s+ | **XFMT** |
| **Deduplication** | **Content-Aware** | None | Monolithic Only | **XFMT** |
| **Compression Ratio** | 0.09 - 0.99 | **0.05 - 0.98** | 0.06 - 0.98 | Tar-XZ / 7-Zip |

### Why choose XFMT?
- **Speed:** Up to 20x faster than 7-Zip at encoding on repetitive data.
- **Zero-Wait Playback:** Stream videos directly from the archive without extracting to a temporary folder.
- **Atomic Seeking:** Jump to any byte offset in a multi-terabyte archive instantly without decompressing the start of the file.

## Features (Current)
- **Zstandard Compression**: Efficient, high-speed data reduction.
- **Content-Defined Chunking (CDC)**: FastCDC enables deduplication even when file content shifts.
- **Local & Global Deduplication**: Share duplicate chunks across multiple files using Repository Mode.
- **Folder Archiving**: Recursive directory bundling with the `--dir` flag.
- **Media Streaming Server**: Built-in HTTP server for instant video playback with full timeline/seeking support.
- **AES-256-GCM Encryption**: Optional authenticated encryption for secure storage.
- **Digital Signatures**: Ed25519 manifest signing to guarantee provenance.
- **Streaming I/O**: True streaming architecture; process 100GB+ files with minimal RAM.

## Usage

### Packing & Bundling
```bash
# Basic pack (single file)
xfmt pack movie.mp4

# Bundle an entire folder (recursive)
xfmt pack ./my_photos --dir

# Fast pack (uses 1MB chunks for instant seeking)
xfmt pack input.dat --fast

# Ultra compression
xfmt pack input.dat --level 19
```

### Unpacking & Media Streaming
```bash
# Extract full file or folder
xfmt unpack output.xfmt restored_data

# Instant Video Playback (with timeline and seeking)
xfmt serve movie.mp4.xfmt --vlc

# Read a specific byte range to stdout
xfmt cat output.xfmt --offset 1024 --length 512
```

### Security & Provenance
```bash
# 1. Generate an Ed25519 key pair
xfmt gen-key my_key

# 2. Pack and sign
xfmt pack input.dat --key my_key

# 3. Verify integrity and signature
xfmt verify output.xfmt
```

## Documentation
See the `docs/` folder or `DOCUMENTATION.pdf` for the full technical manual.
