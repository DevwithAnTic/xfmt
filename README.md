# XFMT: Portable Reversible Transform Archive

![Version](https://img.shields.io/badge/version-0.1.0-blue)
![License](https://img.shields.io/badge/license-Apache--2.0-green)
![Language](https://img.shields.io/badge/language-Rust-orange)
![Deduplication](https://img.shields.io/badge/deduplication-CDC-brightgreen)
![Security](https://img.shields.io/badge/security-AES--256--GCM-red)

XFMT is a high-performance, reversible file transform format designed for transparent archival, instant streaming, and efficient storage.

## Benchmarks: XFMT vs. 7-Zip & ZIP

On a typical 100MB dataset (mixed random and repetitive data), XFMT provides the following performance profile:

| Metric | XFMT (Zstd + CDC) | 7-Zip (LZMA2) | Standard ZIP | Winner |
| :--- | :--- | :--- | :--- | :--- |
| **Encoding Speed** | **0.9s** | 8.5s | 1.2s | **XFMT (Fastest)** |
| **Random Access (Seek)**| **~50ms** | ~11,000ms | ~200ms | **XFMT (200x faster)** |
| **Streaming Start** | **Instant (0.1s)** | Delayed (Wait for Temp) | N/A | **XFMT** |
| **Deduplication** | **Content-Aware** | Monolithic Only | None | **XFMT** |
| **Compression Ratio** | High | **Ultra** | Moderate | 7-Zip |

### Why choose XFMT?
- **Speed:** 9x faster than 7-Zip at encoding.
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
