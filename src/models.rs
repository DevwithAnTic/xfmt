use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Manifest {
    pub format: String,
    pub version: String,
    pub object_id: String,
    pub original_name: String,
    pub original_type: String,
    pub original_size: u64,
    pub transform_profile: String,
    pub chunking: ChunkingPolicy,
    pub integrity: IntegrityPolicy,
    pub security: SecurityPolicy,
    pub compatibility: CompatibilityFlags,
    pub parity: Option<ParityConfig>,
    pub repository_path: Option<String>,
    pub signature: Option<String>, // Hex encoded Ed25519 signature
    pub public_key: Option<String>, // Hex encoded public key
    pub media_info: Option<MediaInfo>, // Image/Video technical specs
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MediaInfo {
    pub width: usize,
    pub height: usize,
    pub format: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ParityConfig {
    pub data_shards: usize,
    pub parity_shards: usize,
    pub shard_size: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChunkingPolicy {
    pub mode: String,
    pub min_size: u32,
    pub avg_size: u32,
    pub max_size: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IntegrityPolicy {
    pub hash: String,
    pub tree: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SecurityPolicy {
    pub encrypted: bool,
    pub signed: bool,
    pub salt: Option<String>, // Hex encoded salt for key derivation
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CompatibilityFlags {
    pub fallback_payload: bool,
    pub legacy_preview: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IndexEntry {
    pub chunk_id: String,
    pub source_offset: u64,
    pub source_length: u64,
    pub stored_offset: u64,
    pub stored_length: u64,
    pub codec: String,
    pub digest: String,
    pub nonce: Option<String>, // Hex encoded nonce for AES-GCM
}

pub const MAGIC: &[u8; 4] = b"XFMT";
pub const VERSION_MAJOR: u16 = 0;
pub const VERSION_MINOR: u16 = 1;
