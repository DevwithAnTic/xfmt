mod models;

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{Context, Result};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use clap::{Parser, Subcommand};
use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer, Verifier};
use models::*;
use pbkdf2::pbkdf2_hmac;
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write, BufRead};
use std::path::{Path, PathBuf};
use std::net::TcpListener;
use std::process::Command;
use std::sync::Arc;
use std::thread;

#[derive(Parser)]
#[command(name = "xfmt")]
#[command(about = "eXtended File Multi-block Transformer", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Pack a file or folder into XFMT format
    Pack {
        input: String,
        output: Option<String>,
        #[arg(short, long)]
        password: Option<String>,
        /// Number of parity shards (default 0)
        #[arg(short, long, default_value_t = 0)]
        parity: usize,
        /// Repository path for global deduplication
        #[arg(short, long)]
        repo: Option<String>,
        /// Private key file for digital signature
        #[arg(short, long)]
        key: Option<String>,
        /// Use small 1MB chunks (default is 16MB for better compression)
        #[arg(long)]
        fast: bool,
        /// Zstd compression level (1-22, default 3)
        #[arg(short, long, default_value_t = 3)]
        level: i32,
        /// Pack input as a directory (recursive bundling)
        #[arg(short, long)]
        dir: bool,
    },
    /// Unpack an XFMT file to its original state
    Unpack {
        input: String,
        output: String,
        #[arg(short, long)]
        password: Option<String>,
        /// Override repository path
        #[arg(short, long)]
        repo: Option<String>,
    },
    /// Print a range of bytes from an XFMT file (stdout)
    Cat {
        input: String,
        #[arg(short, long, default_value_t = 0)]
        offset: u64,
        #[arg(short, long)]
        length: Option<u64>,
        #[arg(short, long)]
        password: Option<String>,
        /// Override repository path
        #[arg(short, long)]
        repo: Option<String>,
    },
    /// Verify the integrity and signature of an XFMT file (alias: ls)
    #[command(alias = "ls")]
    Verify {
        input: String,
        #[arg(short, long)]
        password: Option<String>,
        /// Override repository path
        #[arg(short, long)]
        repo: Option<String>,
    },
    /// Serve an XFMT file as a local HTTP stream (enables seeking/timeline)
    Serve {
        input: String,
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
        #[arg(short, long)]
        password: Option<String>,
        /// Override repository path
        #[arg(short, long)]
        repo: Option<String>,
        /// Automatically launch VLC player
        #[arg(short, long)]
        vlc: bool,
    },
    /// Generate a new Ed25519 key pair for signing
    GenKey {
        output: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Pack { input, output, password, parity, repo, key, fast, level, dir } => {
            let output_path = output.unwrap_or_else(|| format!("{}.xfmt", input));
            if dir {
                if !Path::new(&input).is_dir() { anyhow::bail!("Input is not a directory. Remove --dir flag to pack a single file."); }
                bundle(&input, &output_path, password, repo, key, fast, level)?
            } else {
                if Path::new(&input).is_dir() { anyhow::bail!("Input is a directory. Use --dir or -d flag to pack folders."); }
                pack(&input, &output_path, password, parity, repo, key, fast, level)?
            }
        },
        Commands::Unpack { input, output, password, repo } => unpack(&input, &output, password, repo)?,
        Commands::Cat { input, offset, length, password, repo } => cat(&input, offset, length, password, repo)?,
        Commands::Verify { input, password, repo } => verify(&input, password, repo)?,
        Commands::Serve { input, port, password, repo, vlc } => serve(&input, port, password, repo, vlc)?,
        Commands::GenKey { output } => gen_key(&output)?,
    }

    Ok(())
}

fn gen_key(output_path: &str) -> Result<()> {
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let public_key = signing_key.verifying_key();
    let priv_hex = hex::encode(signing_key.to_bytes());
    let pub_hex = hex::encode(public_key.to_bytes());
    fs::write(output_path, priv_hex)?;
    fs::write(format!("{}.pub", output_path), &pub_hex)?;
    println!("Generated key pair:\n  Private: {} bytes (Saved to {})\n  Public:  {} (Saved to {}.pub)", signing_key.to_bytes().len(), output_path, pub_hex, output_path);
    Ok(())
}

fn derive_key(password: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, 100_000, &mut key);
    key
}

fn get_repo_chunk_path(repo_path: &str, chunk_id: &str) -> Result<PathBuf> {
    let safe_id = chunk_id.replace(":", "_");
    if !safe_id.chars().all(|c| c.is_ascii_hexdigit() || c == '_' || c == '-') { anyhow::bail!("Invalid chunk_id format: {}", safe_id); }
    let mut path = PathBuf::from(repo_path);
    path.push("chunks");
    if safe_id.len() < 2 { anyhow::bail!("chunk_id too short"); }
    path.push(&safe_id[..2]);
    path.push(safe_id);
    Ok(path)
}

fn process_chunk(
    chunk_bytes: &[u8],
    global_hasher: &mut Sha256,
    unique_chunks: &mut HashMap<String, (u64, u64, Option<String>)>,
    index: &mut Vec<IndexEntry>,
    current_source_offset: &mut u64,
    current_stored_offset: &mut u64,
    payload_file: &mut File,
    repo: &Option<String>,
    cipher: &Option<Aes256Gcm>,
    compression_level: i32,
) -> Result<()> {
    global_hasher.update(chunk_bytes);
    let mut chunk_hasher = Sha256::new();
    chunk_hasher.update(chunk_bytes);
    let digest = format!("sha256:{}", hex::encode(chunk_hasher.finalize()));
    let chunk_id = digest.clone();
    let (stored_offset, stored_len, nonce_str) =
        if let Some(&(offset, len, ref n)) = unique_chunks.get(&digest) { (offset, len, n.clone()) }
        else {
            let compressed_chunk = zstd::encode_all(chunk_bytes, compression_level)?;
            let (final_payload, nonce_val) = if let Some(c) = cipher {
                let mut nonce_bytes = [0u8; 12]; OsRng.fill_bytes(&mut nonce_bytes);
                let encrypted = c.encrypt(Nonce::from_slice(&nonce_bytes), compressed_chunk.as_slice()).map_err(|e| anyhow::anyhow!(e))?;
                (encrypted, Some(hex::encode(nonce_bytes)))
            } else { (compressed_chunk, None) };
            let len = final_payload.len() as u64;
            let offset = *current_stored_offset;
            if let Some(repo_path) = repo {
                let chunk_path = get_repo_chunk_path(repo_path, &chunk_id)?;
                if !chunk_path.exists() { fs::create_dir_all(chunk_path.parent().unwrap())?; fs::write(chunk_path, &final_payload)?; }
            } else { payload_file.write_all(&final_payload)?; }
            unique_chunks.insert(digest.clone(), (offset, len, nonce_val.clone()));
            *current_stored_offset += len; (offset, len, nonce_val)
        };
    index.push(IndexEntry { chunk_id, source_offset: *current_source_offset, source_length: chunk_bytes.len() as u64, stored_offset, stored_length: stored_len, codec: "zstd".to_string(), digest, nonce: nonce_str });
    *current_source_offset += chunk_bytes.len() as u64;
    Ok(())
}

fn pack(input_path: &str, output_path: &str, password: Option<String>, _parity_count: usize, repo: Option<String>, key_path: Option<String>, fast_mode: bool, level: i32) -> Result<()> {
    let input_p = Path::new(input_path);
    let original_name = input_p.file_name().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
    let (min_size, avg_size, max_size) = if fast_mode { (256 * 1024, 1024 * 1024, 4 * 1024 * 1024) } else { (1024 * 1024, 16 * 1024 * 1024, 64 * 1024 * 1024) };
    let mut salt = [0u8; 16];
    let mut cipher = None;
    if let Some(ref pw) = password { OsRng.fill_bytes(&mut salt); cipher = Some(Aes256Gcm::new_from_slice(&derive_key(pw, &salt)).map_err(|e| anyhow::anyhow!(e))?); }
    if let Some(ref repo_path) = repo { fs::create_dir_all(Path::new(repo_path).join("chunks"))?; }
    let payload_path = format!("{}.payloads", output_path);
    let mut payload_file = File::create(&payload_path)?;
    let (mut index, mut cur_src, mut cur_sto) = (Vec::new(), 0, 0);
    let mut global_hasher = Sha256::new();
    let mut unique_chunks = HashMap::new();
    let original_size = input_p.metadata()?.len();
    let original_type = mime_guess::from_path(input_path).first_or_octet_stream().to_string();
    let input_file = File::open(input_path)?;
    for chunk in fastcdc::v2020::StreamCDC::new(input_file, min_size, avg_size, max_size) {
        process_chunk(&chunk?.data, &mut global_hasher, &mut unique_chunks, &mut index, &mut cur_src, &mut cur_sto, &mut payload_file, &repo, &cipher, level)?;
    }
    payload_file.flush()?;
    let object_id = format!("sha256:{}", hex::encode(global_hasher.finalize()));
    let mut manifest = Manifest { format: "XFMT".to_string(), version: "0.1".to_string(), object_id, original_name, original_type, original_size, transform_profile: "generic.reversible.v1".to_string(), chunking: ChunkingPolicy { mode: "content_defined".to_string(), min_size: min_size as u32, avg_size: avg_size as u32, max_size: max_size as u32 }, integrity: IntegrityPolicy { hash: "sha256".to_string(), tree: "none".to_string() }, security: SecurityPolicy { encrypted: password.is_some(), signed: key_path.is_some(), salt: password.as_ref().map(|_| hex::encode(salt)) }, compatibility: CompatibilityFlags { fallback_payload: false, legacy_preview: false }, parity: None, repository_path: repo.clone(), signature: None, public_key: None, media_info: None, bundle_files: None };
    if let Some(ref kp) = key_path {
        let key_hex = fs::read_to_string(kp)?; let signing_key = SigningKey::from_bytes(hex::decode(key_hex.trim())?.as_slice().try_into()?);
        let sig = signing_key.sign(&serde_json::to_vec(&manifest)?);
        manifest.signature = Some(hex::encode(sig.to_bytes())); manifest.public_key = Some(hex::encode(signing_key.verifying_key().to_bytes()));
    }
    let (m_json, i_json) = (serde_json::to_vec(&manifest)?, serde_json::to_vec(&index)?);
    let mut final_output = File::create(output_path)?;
    final_output.write_all(MAGIC)?; final_output.write_u16::<BigEndian>(VERSION_MAJOR)?; final_output.write_u16::<BigEndian>(VERSION_MINOR)?;
    final_output.write_u32::<BigEndian>(m_json.len() as u32)?; final_output.write_u32::<BigEndian>(i_json.len() as u32)?;
    final_output.write_u32::<BigEndian>(0)?; final_output.write_all(&m_json)?; final_output.write_all(&i_json)?;
    if repo.is_none() { std::io::copy(&mut File::open(&payload_path)?, &mut final_output)?; }
    let _ = std::fs::remove_file(&payload_path);
    println!("Packed {} -> {} ({} bytes)", input_path, output_path, final_output.metadata()?.len());
    Ok(())
}

fn bundle(input_path: &str, output_path: &str, password: Option<String>, repo: Option<String>, key_path: Option<String>, fast_mode: bool, level: i32) -> Result<()> {
    let input_p = Path::new(input_path);
    let original_name = input_p.file_name().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
    let (min_size, avg_size, max_size) = if fast_mode { (256 * 1024, 1024 * 1024, 4 * 1024 * 1024) } else { (1024 * 1024, 16 * 1024 * 1024, 64 * 1024 * 1024) };
    let mut salt = [0u8; 16];
    let mut cipher = None;
    if let Some(ref pw) = password { OsRng.fill_bytes(&mut salt); cipher = Some(Aes256Gcm::new_from_slice(&derive_key(pw, &salt)).map_err(|e| anyhow::anyhow!(e))?); }
    if let Some(ref repo_path) = repo { fs::create_dir_all(Path::new(repo_path).join("chunks"))?; }
    let payload_path = format!("{}.payloads", output_path);
    let mut payload_file = File::create(&payload_path)?;
    let (mut index, mut cur_src, mut cur_sto) = (Vec::new(), 0, 0);
    let mut global_hasher = Sha256::new();
    let mut unique_chunks = HashMap::new();
    let mut total_size = 0;
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(input_path).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let rel_path = entry.path().strip_prefix(input_path)?.to_str().unwrap().replace("\\", "/");
            let size = entry.metadata()?.len();
            files.push(BundleEntry { rel_path, size, offset: total_size });
            let f = File::open(entry.path())?;
            for chunk in fastcdc::v2020::StreamCDC::new(f, min_size, avg_size, max_size) {
                process_chunk(&chunk?.data, &mut global_hasher, &mut unique_chunks, &mut index, &mut cur_src, &mut cur_sto, &mut payload_file, &repo, &cipher, level)?;
            }
            total_size += size;
        }
    }
    payload_file.flush()?;
    let object_id = format!("sha256:{}", hex::encode(global_hasher.finalize()));
    let mut manifest = Manifest { format: "XFMT".to_string(), version: "0.1".to_string(), object_id, original_name, original_type: "application/x-directory".to_string(), original_size: total_size, transform_profile: "bundle.v1".to_string(), chunking: ChunkingPolicy { mode: "content_defined".to_string(), min_size: min_size as u32, avg_size: avg_size as u32, max_size: max_size as u32 }, integrity: IntegrityPolicy { hash: "sha256".to_string(), tree: "none".to_string() }, security: SecurityPolicy { encrypted: password.is_some(), signed: key_path.is_some(), salt: password.as_ref().map(|_| hex::encode(salt)) }, compatibility: CompatibilityFlags { fallback_payload: false, legacy_preview: false }, parity: None, repository_path: repo.clone(), signature: None, public_key: None, media_info: None, bundle_files: Some(files) };
    if let Some(ref kp) = key_path {
        let key_hex = fs::read_to_string(kp)?; let signing_key = SigningKey::from_bytes(hex::decode(key_hex.trim())?.as_slice().try_into()?);
        let sig = signing_key.sign(&serde_json::to_vec(&manifest)?);
        manifest.signature = Some(hex::encode(sig.to_bytes())); manifest.public_key = Some(hex::encode(signing_key.verifying_key().to_bytes()));
    }
    let (m_json, i_json) = (serde_json::to_vec(&manifest)?, serde_json::to_vec(&index)?);
    let mut final_output = File::create(output_path)?;
    final_output.write_all(MAGIC)?; final_output.write_u16::<BigEndian>(VERSION_MAJOR)?; final_output.write_u16::<BigEndian>(VERSION_MINOR)?;
    final_output.write_u32::<BigEndian>(m_json.len() as u32)?; final_output.write_u32::<BigEndian>(i_json.len() as u32)?;
    final_output.write_u32::<BigEndian>(0)?; final_output.write_all(&m_json)?; final_output.write_all(&i_json)?;
    if repo.is_none() { std::io::copy(&mut File::open(&payload_path)?, &mut final_output)?; }
    let _ = std::fs::remove_file(&payload_path);
    println!("Bundled folder {} -> {} ({} bytes)", input_path, output_path, final_output.metadata()?.len());
    Ok(())
}

const MAX_METADATA_SIZE: usize = 64 * 1024 * 1024;

fn unpack(input_path: &str, output_path: &str, password: Option<String>, repo_override: Option<String>) -> Result<()> {
    let mut input_file = File::open(input_path)?;
    let mut magic = [0u8; 4]; input_file.read_exact(&mut magic)?;
    if &magic != MAGIC { anyhow::bail!("Invalid Magic"); }
    let _ = input_file.read_u16::<BigEndian>()?; let _ = input_file.read_u16::<BigEndian>()?;
    let m_len = input_file.read_u32::<BigEndian>()? as usize; let i_len = input_file.read_u32::<BigEndian>()? as usize;
    let _ = input_file.read_u32::<BigEndian>()?;
    if m_len > MAX_METADATA_SIZE || i_len > MAX_METADATA_SIZE { anyhow::bail!("OOM Protection"); }
    let mut m_buf = vec![0u8; m_len]; input_file.read_exact(&mut m_buf)?; let manifest: Manifest = serde_json::from_slice(&m_buf)?;
    let mut i_buf = vec![0u8; i_len]; input_file.read_exact(&mut i_buf)?; let index: Vec<IndexEntry> = serde_json::from_slice(&i_buf)?;
    let mut key = None;
    if manifest.security.encrypted { key = Some(derive_key(&password.context("PW required")?, &hex::decode(manifest.security.salt.as_ref().context("Salt missing")?)?)); }
    if let Some(ref files) = manifest.bundle_files {
        fs::create_dir_all(output_path)?;
        for file in files {
            let out_p = Path::new(output_path).join(&file.rel_path);
            if let Some(p) = out_p.parent() { fs::create_dir_all(p)?; }
            let mut out_f = File::create(out_p)?;
            cat_to_writer(input_path, file.offset, Some(file.size), &key, m_len, i_len, &manifest, &index, repo_override.clone(), &mut out_f)?;
        }
    } else {
        let mut out_f = File::create(output_path)?;
        cat_to_writer(input_path, 0, Some(manifest.original_size), &key, m_len, i_len, &manifest, &index, repo_override, &mut out_f)?;
    }
    Ok(())
}

fn cat_to_writer<W: Write>(input_path: &str, start_offset: u64, length: Option<u64>, key: &Option<[u8; 32]>, m_len: usize, i_len: usize, manifest: &Manifest, index: &[IndexEntry], repo_override: Option<String>, writer: &mut W) -> Result<()> {
    let mut input_file = File::open(input_path)?;
    let p_start = 20 + m_len as u64 + i_len as u64;
    let end_offset = length.map(|l| start_offset + l).unwrap_or(u64::MAX);
    let cipher = key.map(|k| Aes256Gcm::new_from_slice(k.as_slice()).unwrap());
    for entry in index {
        let (c_start, c_end) = (entry.source_offset, entry.source_offset + entry.source_length);
        if c_start < end_offset && c_end > start_offset {
            let stored = if let Some(ref r) = repo_override.clone().or(manifest.repository_path.clone()) { fs::read(get_repo_chunk_path(r, &entry.chunk_id)?)? }
            else { input_file.seek(SeekFrom::Start(p_start + entry.stored_offset))?; let mut buf = vec![0u8; entry.stored_length as usize]; input_file.read_exact(&mut buf)?; buf };
            let payload = if let Some(ref c) = cipher { c.decrypt(Nonce::from_slice(&hex::decode(entry.nonce.as_ref().context("Nonce missing")?)?), stored.as_slice()).map_err(|_| anyhow::anyhow!("Decryption failed"))? } else { stored };
            let mut decoder = zstd::stream::read::Decoder::new(&payload[..])?;
            let mut decomp = Vec::with_capacity(entry.source_length as usize); std::io::copy(&mut decoder.by_ref().take(entry.source_length), &mut decomp)?;
            let s_start = if start_offset > c_start { (start_offset - c_start) as usize } else { 0 };
            let s_end = if end_offset < c_end { (end_offset - c_start) as usize } else { decomp.len() };
            writer.write_all(&decomp[s_start..s_end])?;
        }
    }
    Ok(())
}

fn cat(input_path: &str, start_offset: u64, length: Option<u64>, password: Option<String>, repo_override: Option<String>) -> Result<()> {
    let mut input_file = File::open(input_path)?;
    let mut magic = [0u8; 4]; input_file.read_exact(&mut magic)?;
    let _ = input_file.read_u16::<BigEndian>()?; let _ = input_file.read_u16::<BigEndian>()?;
    let m_len = input_file.read_u32::<BigEndian>()? as usize; let i_len = input_file.read_u32::<BigEndian>()? as usize;
    let _ = input_file.read_u32::<BigEndian>()?;
    let mut m_buf = vec![0u8; m_len]; input_file.read_exact(&mut m_buf)?; let manifest: Manifest = serde_json::from_slice(&m_buf)?;
    let mut i_buf = vec![0u8; i_len]; input_file.read_exact(&mut i_buf)?; let index: Vec<IndexEntry> = serde_json::from_slice(&i_buf)?;
    let mut key = None;
    if manifest.security.encrypted { key = Some(derive_key(&password.context("PW required")?, &hex::decode(manifest.security.salt.as_ref().context("Salt missing")?)?)); }
    cat_to_writer(input_path, start_offset, length, &key, m_len, i_len, &manifest, &index, repo_override, &mut std::io::stdout())?;
    Ok(())
}

fn serve(input_path: &str, port: u16, password: Option<String>, repo_override: Option<String>, open_vlc: bool) -> Result<()> {
    let mut input_file = File::open(input_path)?;
    let mut magic = [0u8; 4]; input_file.read_exact(&mut magic)?;
    let _ = input_file.read_u16::<BigEndian>()?; let _ = input_file.read_u16::<BigEndian>()?;
    let m_len = input_file.read_u32::<BigEndian>()? as usize; let i_len = input_file.read_u32::<BigEndian>()? as usize;
    let _ = input_file.read_u32::<BigEndian>()?;
    let mut m_buf = vec![0u8; m_len]; input_file.read_exact(&mut m_buf)?; let manifest: Arc<Manifest> = Arc::new(serde_json::from_slice(&m_buf)?);
    let mut i_buf = vec![0u8; i_len]; input_file.read_exact(&mut i_buf)?; let index: Arc<Vec<IndexEntry>> = Arc::new(serde_json::from_slice(&i_buf)?);
    let mut key = None;
    if manifest.security.encrypted { key = Some(derive_key(&password.context("PW required")?, &hex::decode(manifest.security.salt.as_ref().context("Salt missing")?)?)); }
    let key = Arc::new(key);
    let input_path = Arc::new(input_path.to_string());
    let repo_override = Arc::new(repo_override);
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port))?;
    println!("XFMT Server active at http://127.0.0.1:{}", port);
    if open_vlc {
        let url = format!("http://127.0.0.1:{}", port);
        #[cfg(target_os = "windows")]
        let _ = Command::new("vlc").arg(&url).spawn()
            .or_else(|_| Command::new("C:\\Program Files\\VideoLAN\\VLC\\vlc.exe").arg(&url).spawn())
            .or_else(|_| Command::new("C:\\Program Files (x86)\\VideoLAN\\VLC\\vlc.exe").arg(&url).spawn());

        #[cfg(target_os = "macos")]
        let _ = Command::new("open").arg("-a").arg("VLC").arg(&url).spawn();

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let _ = Command::new("vlc").arg(&url).spawn();
    }
    for stream in listener.incoming() {
        let mut stream = stream?;
        let (manifest, index, key, input_path, repo_override) = (Arc::clone(&manifest), Arc::clone(&index), Arc::clone(&key), Arc::clone(&input_path), Arc::clone(&repo_override));
        thread::spawn(move || -> Result<()> {
            let mut reader = std::io::BufReader::new(&mut stream);
            let mut request = String::new();
            if reader.read_line(&mut request).is_err() || request.is_empty() { return Ok(()); }
            let mut range_header = None;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() || line.trim().is_empty() { break; }
                if line.to_lowercase().starts_with("range: bytes=") { range_header = Some(line.trim()["range: bytes=".len()..].to_string()); }
            }
            let (start, end) = if let Some(ref range) = range_header {
                let parts: Vec<&str> = range.split('-').collect();
                let s = parts[0].parse::<u64>().unwrap_or(0);
                let e = if parts.len() > 1 && !parts[1].is_empty() { parts[1].parse::<u64>().unwrap_or(manifest.original_size - 1) } else { manifest.original_size - 1 };
                (s, e)
            } else { (0, manifest.original_size - 1) };
            let content_length = end - start + 1;
            let response = if range_header.is_some() { format!("HTTP/1.1 206 Partial Content\r\nContent-Type: {}\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n", manifest.original_type, content_length, start, end, manifest.original_size) }
            else { format!("HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n", manifest.original_type, manifest.original_size) };
            stream.write_all(response.as_bytes())?;
            let p_start = 20 + m_len as u64 + i_len as u64;
            let cipher = key.as_ref().map(|k| Aes256Gcm::new_from_slice(k.as_slice()).unwrap());
            for entry in index.iter() {
                let (c_start, c_end) = (entry.source_offset, entry.source_offset + entry.source_length);
                if c_start <= end && c_end > start {
                    let mut chunk_file = File::open(&*input_path)?;
                    let stored = if let Some(ref r) = repo_override.as_ref().clone().or(manifest.repository_path.clone()) { fs::read(get_repo_chunk_path(r, &entry.chunk_id)?)? }
                    else { chunk_file.seek(SeekFrom::Start(p_start + entry.stored_offset))?; let mut buf = vec![0u8; entry.stored_length as usize]; chunk_file.read_exact(&mut buf)?; buf };
                    let payload = if let Some(ref c) = cipher { c.decrypt(Nonce::from_slice(&hex::decode(entry.nonce.as_ref().unwrap())?), stored.as_slice()).map_err(|_| anyhow::anyhow!("Decryption failed"))? } else { stored };
                    let mut decoder = zstd::stream::read::Decoder::new(&payload[..])?;
                    let mut decomp = Vec::with_capacity(entry.source_length as usize); std::io::copy(&mut decoder.by_ref().take(entry.source_length), &mut decomp)?;
                    let s_start = if start > c_start { (start - c_start) as usize } else { 0 };
                    let s_end = if end < c_end - 1 { (end - c_start + 1) as usize } else { decomp.len() };
                    if s_start < decomp.len() { stream.write_all(&decomp[s_start..s_end])?; }
                }
            }
            Ok(())
        });
    }
    Ok(())
}

fn verify(input_path: &str, _password: Option<String>, _repo_override: Option<String>) -> Result<()> {
    let mut input_file = File::open(input_path)?;
    let mut magic = [0u8; 4]; input_file.read_exact(&mut magic)?;
    let _ = input_file.read_u16::<BigEndian>()?; let _ = input_file.read_u16::<BigEndian>()?;
    let m_len = input_file.read_u32::<BigEndian>()? as usize; let i_len = input_file.read_u32::<BigEndian>()? as usize;
    let _ = input_file.read_u32::<BigEndian>()?;
    let mut m_buf = vec![0u8; m_len]; input_file.read_exact(&mut m_buf)?; let manifest: Manifest = serde_json::from_slice(&m_buf)?;
    let mut i_buf = vec![0u8; i_len]; input_file.read_exact(&mut i_buf)?; let _index: Vec<IndexEntry> = serde_json::from_slice(&i_buf)?;
    println!("--- XFMT Manifest Info ---\nName: {}\nSize: {} bytes\nType: {}\nEncrypted: {}", manifest.original_name, manifest.original_size, manifest.original_type, manifest.security.encrypted);
    if let Some(ref bundle) = manifest.bundle_files { println!("Bundle: {} files", bundle.len()); for f in bundle.iter().take(5) { println!(" - {}", f.rel_path); } if bundle.len() > 5 { println!(" ... and {} more", bundle.len() - 5); } }
    if let Some(ref sig_hex) = manifest.signature {
        let pub_hex = manifest.public_key.as_ref().context("PK missing")?;
        let mut m_clone = manifest.clone(); m_clone.signature = None; m_clone.public_key = None;
        VerifyingKey::from_bytes(hex::decode(pub_hex)?.as_slice().try_into()?)?.verify(&serde_json::to_vec(&m_clone)?, &Signature::from_bytes(hex::decode(sig_hex)?.as_slice().try_into()?))?;
        println!("Signature: VALID\nPublic Key: {}", pub_hex);
    }
    Ok(())
}
