# XFMT Technical Manual: Deep Dive into Reversible Archiving

## Table of Contents
1. **Introduction & Core Concepts**
2. **The XFMT Binary Specification**
3. **The Packing Pipeline**
4. **Unpacking & Random Access**
5. **Repository Mode & Global Deduplication**
6. **Security & Cryptography**
7. **The Streaming Server (VLC Integration)**
8. **Performance & Optimization**

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

## Chapter 3: The Packing Pipeline

The `pack` function in `src/main.rs` follows a streaming architecture:

1. **CDC Streamer:** The input file is read in chunks. It doesn't load the whole file into RAM.
2. **The Deduplicator:** It checks if a chunk's hash already exists in the current archive (Local) or a `--repo` (Global).
3. **The Compressor:** New chunks are compressed with Zstd.
4. **The Encryptor:** If a password is set, chunks are encrypted with **AES-256-GCM**.
5. **The Finalizer:** It writes the Header, then the JSON metadata, and finally appends the compressed payloads.

---

## Chapter 4: Unpacking & Random Access

### 4.1 Full Unpack
The `unpack` function reads the index and iterates through chunks. It seeks directly to each `stored_offset`, decompresses, and writes to the output file.

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

The development of XFMT followed a structured, 9-phase roadmap to ensure stability and feature-richness.

### Phase 1: Core Foundation
- Initial setup and binary header definition.
- Implementation of the basic Zstd packing/unpacking pipeline.

### Phase 2: Random Access & Content Addressing
- Switched to SHA-256 chunk IDs.
- Implemented lazy decompression for instant seeking.
- Added local deduplication.

### Phase 3: Security & Encryption
- Integrated AES-256-GCM authenticated encryption.
- Added PBKDF2 key derivation from passwords.

### Phase 4: Reliability & Parity
- Added Reed-Solomon erasure coding to protect against data corruption.

### Phase 5: Streaming I/O (Scalability)
- Refactored core logic to process data in chunks without loading full files into memory.

### Phase 6: Content-Defined Chunking (CDC)
- Integrated FastCDC for content-aware boundaries.
- Optimized for "shifting data" (insertions/deletions) to maximize deduplication.

### Phase 7: Repository Mode (Global Deduplication)
- Created a centralized chunk store (Repository) to share data across multiple archives.

### Phase 8: Type-Aware Profiles
- Added smart handling for JSON (canonicalization) and Images (metadata extraction).

### Phase 9: Signatures & Hardening
- Implemented Ed25519 digital signatures for provenance.
- Built a multi-threaded streaming server for media playback.
- Hardened the system against DoS (Zip Bomb) and path traversal attacks.

---

## Chapter 10: System Architecture

The following diagram illustrates the flow of data through the XFMT system during a `pack` and `serve` operation.

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
    |   CDC Chunker     +----> |   Type-Aware Profile  |
    |   (FastCDC)       |      |   (JSON/Image Specs)  |
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

### 10.2 Component Responsibilities
- **CLI:** Parses user intent and manages passwords/keys.
- **CDC Chunker:** Performs content-defined slicing to ensure deduplication survives byte shifts.
- **Global Repository:** A content-addressed store that allows multiple archives to share the same data blocks.
- **Security Layer:** Handles authenticated encryption (GCM) ensuring data hasn't been modified at rest.
- **HTTP Bridge:** Translates "Range Requests" from media players into atomic chunk decompressions.

---

*Manual generated for XFMT v0.1.0*
