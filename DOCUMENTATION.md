# XFMT Technical Manual: eXtended File Multi-block Transformer

## Table of Contents
1. **Introduction & Core Concepts**
2. **The XFMT Binary Specification**
3. **The Packing & Bundling Pipelines**
4. **Unpacking & Random Access**
5. **Repository Mode & Global Deduplication**
6. **Security & Cryptography**
7. **The Streaming Server (VLC Integration)**
8. **Performance & Optimization**
9. **Project Development Workflow**
10. **System Architecture**

---

## Chapter 1: Introduction & Core Concepts

XFMT (eXtendable Format for Media and Transforms) is designed to solve the "Monolithic Archive Problem." Unlike ZIP or 7z, which require full extraction for access, XFMT treats an archive as a **database of chunks**.

### 1.1 Content-Defined Chunking (CDC)
Standard chunking (fixed size) breaks if you add one byte to the start of a file (all boundaries shift). XFMT uses **FastCDC**, which "looks" at the data and sets boundaries based on content patterns.
- **Benefit:** If you change one frame in a video, 99.9% of the other chunks remain identical and can be deduplicated.

### 1.2 The Reversibility Guarantee
XFMT is bit-perfect. It doesn't just store "data"; it stores the exact byte-sequence. Using Zstandard (Zstd), it compresses data at speeds exceeding 500MB/s while maintaining the ability to restore the original file hash exactly.

---

## Chapter 2: The XFMT Binary Specification

An XFMT file is structured for **Instant Seek**. You don't read the file from start to finish; you jump to the metadata.

### 2.1 The Header (20 Bytes)
| Offset | Size | Purpose |
| :--- | :--- | :--- |
| 0 | 4 | Magic Bytes `b"XFMT"` |
| 4 | 2 | Major Version |
| 6 | 2 | Minor Version |
| 8 | 4 | Manifest Length (u32) |
| 12 | 4 | Index Length (u32) |
| 16 | 4 | Flags (u32) |

### 2.2 The Manifest (JSON)
Stored immediately after the header. Contains global metadata:
- `original_name`, `original_size`, `mime_type`.
- Security settings (Salt, Encryption status).
- Digital Signature & Public Key.
- `bundle_files`: File map (only used with `--dir` flag).

### 2.3 The Index (JSON Array)
A map of every chunk in the file.
```json
{
  "chunk_id": "sha256:...",
  "source_offset": 0,
  "source_length": 1048576,
  "stored_offset": 0,
  "stored_length": 450123
}
```
Because the index is at the **start** of the file, XFMT knows exactly where any byte is located without scanning the payloads.

---

## Chapter 3: The Packing & Bundling Pipelines

XFMT provides two distinct modes for data ingest, both consolidated under the `pack` command:

### 3.1 `pack <FILE>` (Single File Mode)
The default mode optimized for processing a single large file. It applies type-aware transformations (like Image metadata extraction) and creates a direct 1:1 mapping between chunks and the source file.

### 3.2 `pack <FOLDER> --dir` (Folder Mode)
Triggered by the `--dir` or `-d` flag. It recursively walks a directory tree, treating the entire folder contents as a single continuous stream for maximum CDC deduplication efficiency, while maintaining a `bundle_files` manifest to allow perfect restoration of the folder structure.

---

## Chapter 4: Unpacking & Random Access

### 4.1 Full Unpack
The `unpack` function reads the index and iterates through chunks. It seeks directly to each `stored_offset`, decompresses, and writes to the output file. If the manifest contains a bundle map, it automatically recreates the subdirectory structure.

### 4.2 The `cat` Command (Atomic Seek)
The `cat` command demonstrates XFMT's power. To read 1KB from the middle of a 100GB file:
1. It looks at the index to find which chunk contains that 1KB.
2. It seeks **only** to that chunk.
3. It decompresses **only** that chunk.
- **Result:** Access time is identical regardless of file size.

---

## Chapter 5: Repository Mode & Global Deduplication

By using `--repo`, XFMT moves chunks out of the `.xfmt` file and into a centralized folder.
- **Storage:** Chunks are stored as `chunks/ab/abcdef123...`.
- **Logic:** Multiple `.xfmt` files can point to the same chunk. If you have 10 versions of the same movie, the "base" data is only stored once.

---

## Chapter 6: Security & Cryptography

XFMT implements a "Zero Trust" storage model.

### 6.1 Encryption
- **Key Derivation:** PBKDF2 with 100,000 iterations of HMAC-SHA256.
- **Algorithm:** AES-256-GCM (Authenticated Encryption).
- **Nonce:** Every single chunk has a unique 12-byte random nonce stored in the index. This prevents "frequency analysis" attacks.

### 6.2 Digital Signatures
Uses **Ed25519** (Edwards-curve Digital Signature Algorithm).
1. The Manifest is serialized to JSON.
2. The private key signs the JSON.
3. The signature is embedded in the file.
- **Verification:** `xfmt verify` confirms the file hasn't been tampered with by an attacker.

---

## Chapter 7: The Streaming Server (VLC Integration)

The `serve` command acts as a **Translation Bridge**.

1. It starts a multi-threaded TCP server on `127.0.0.1:8080`.
2. It "tricks" VLC into thinking it's talking to a normal web server.
3. When VLC asks for a "Range" (e.g., `Range: bytes=500-600MB`), the server:
   - Identifies the chunks needed.
   - Decompresses them on the fly.
   - Streams the raw bytes to VLC.
- **Impact:** This allows timeline seeking in encrypted archives without ever writing a single byte to the "Temp" folder.

---

## Chapter 8: Performance & Optimization

### 8.1 Chunk Tuning
- **16MB (Default):** Best for video. High compression ratio, fast seeking.
- **1MB (`--fast`):** Best for documents or low-RAM devices. Ultra-fast seeking.

### 8.2 Memory Safety
XFMT implements **OOM (Out of Memory) Protection**:
- Metadata (Manifest/Index) is capped at **64MB**.
- Decompressed chunks are capped at **100MB**.
- This prevents "Decompression Bombs" from crashing the system.

---

## Chapter 9: Project Development Workflow

The development of XFMT followed a structured, 10-phase roadmap to ensure stability and feature-richness.

### Phases 1-9: Core Engine
- Implementation of the binary format, CDC, encryption, RS parity, streaming I/O, and media server.

### Phase 10: Folder Archiving (Bundling)
- Implemented recursive directory walking using the `walkdir` crate.
- Integrated folder support into the `pack` command via the `--dir` flag.
- Enhanced `unpack` to automatically recreate folder structures and restore files.

---

## Chapter 10: System Architecture

### 10.1 Data Flow Diagram
```text
       [ USER INPUT ]
             |
             v
    +-------------------+
    |   CLI (clap)      | <--- (Commands: pack, unpack, serve, cat)
    +---------+---------+
              |
      [ STREAMING PIPELINE ]
              |
              v
    +-------------------+      +-----------------------+
    |   CDC Chunker     +----> |   Bundle Mapper       |
    |   (FastCDC)       |      |   (Folder Recursive)  |
    +---------+---------+      +-----------------------+
              |
              v
    +-------------------+      +-----------------------+
    |   Deduplicator    +<---->+   Global Repository   |
    |   (SHA-256 Map)   |      |   (chunks/xx/hash)    |
    +---------+---------+      +-----------------------+
              |
              v
    +-------------------+      +-----------------------+
    |   Transform Layer |      |   Security Layer      |
    |   (Zstd @ lvl 3)  +----> |   (AES-256-GCM)       |
    +---------+---------+      +-----------+-----------+
              |                            |
              v                            v
    +--------------------------------------+-----------+
    |             XFMT ARCHIVE FILE (.xfmt)            |
    |  [Header] [Manifest] [Index] [Compressed Chunks] |
    +--------------------------------------+-----------+
              |
              +----------- [ serve ] -----------+
                                                |
                                                v
                                      +-------------------+
                                      |  HTTP/TCP Bridge  |
                                      |  (Multi-threaded) |
                                      +---------+---------+
                                                |
                                                v
                                      +-------------------+
                                      |    VLC Player     |
                                      |  (Instant Stream) |
                                      +-------------------+
```

---

*Manual generated for XFMT v0.1.0*
