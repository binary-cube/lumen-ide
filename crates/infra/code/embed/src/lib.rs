//! Static model2vec embeddings (default `minishlab/potion-code-16M`, MIT): each
//! token has a vector; a text's vector is the weighted mean of its tokens' vectors,
//! normalized. No neural network, no GPU: fast and identical on all platforms.
//!
//! The vector matrix (64 MB) is not read into memory: it is mapped from the file, and
//! the OS loads only the pages in use.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use text_search::Embedder;

/// The default model and its files.
pub const MODEL: &str = "minishlab/potion-code-16M";
/// The model's revision downloaded (a commit: a branch could change under us).
const REVISION: &str = "1b0ff71095656b23306542bbad34a09109673720";
/// Each file of the model at [`REVISION`]: its name, size and SHA-256. A download that
/// doesn't match is thrown away.
const FILES: &[(&str, u64, &str)] = &[
    ("config.json", 97, "edf07552b5d768d556ded176d19f3a34f25360548de3a246d226ce8e28647914"),
    (
        "tokenizer.json",
        1_041_917,
        "8e84217af15e70e8127c855435fc3d8a4cd91d7bbe686f72e75f188118ec78ae",
    ),
    (
        "model.safetensors",
        64_299_272,
        "ca6159081a6e96cebe4ad878e5e8437bfccc761e8db16223370149cd2faa6c0b",
    ),
];
/// How many tokens of a text count (code chunks are short).
const MAX_TOKENS: usize = 1024;

/// A loaded model.
pub struct Model2Vec {
    vocab: HashMap<String, u32>,
    unk: Option<u32>,
    /// Token → row in the matrix (models with quantized vocabulary) and its weight.
    mapping: Vec<u32>,
    weights: Vec<f32>,
    matrix: memmap2::Mmap,
    /// Where the vectors start in the file (bytes) and how many dimensions they have.
    offset: usize,
    dims: usize,
    rows: usize,
    normalize: bool,
}

impl std::fmt::Debug for Model2Vec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Model2Vec")
            .field("vocab", &self.vocab.len())
            .field("dims", &self.dims)
            .finish()
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// The model directory, inside the given config directory (e.g. `~/.config/<app id>`).
pub fn model_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("models").join(MODEL.replace('/', "--"))
}

/// The model is fully downloaded (each file whole: its size).
pub fn is_installed(dir: &Path) -> bool {
    FILES.iter().all(|(name, size, _)| {
        std::fs::metadata(dir.join(name)).is_ok_and(|m| m.is_file() && m.len() == *size)
    })
}

/// Downloads the model from Hugging Face (only once). `progress(received, total)`.
pub fn download(dir: &Path, progress: &dyn Fn(u64, u64)) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for &(file, size, sha256) in FILES {
        let target = dir.join(file);
        if std::fs::metadata(&target).is_ok_and(|m| m.is_file() && m.len() == size) {
            continue;
        }
        let url = format!("https://huggingface.co/{MODEL}/resolve/{REVISION}/{file}");
        let response =
            ureq::get(&url).call().map_err(|e| io::Error::other(format!("{url}: {e}")))?;
        let total = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(size);
        let mut reader = response.into_body().into_reader();
        // Temporary file, renamed at the end: an interrupted download leaves no broken model.
        let part = target.with_extension("part");
        let mut out = File::create(&part)?;
        let mut buffer = vec![0u8; 1 << 16];
        let mut received = 0u64;
        let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
        loop {
            let n = reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            received += n as u64;
            // Never more than the file is: a server sending on forever fills no disk.
            if received > size {
                break;
            }
            out.write_all(&buffer[..n])?;
            digest.update(&buffer[..n]);
            progress(received, total);
        }
        out.flush()?;
        drop(out);
        let hash: String = digest.finish().as_ref().iter().map(|b| format!("{b:02x}")).collect();
        // Cut short (a dropped connection ends like a whole file) or another file: thrown
        // away, downloaded again next time.
        if received != size || hash != sha256 {
            let _ = std::fs::remove_file(&part);
            return Err(invalid(format!("{file}: the download doesn't match the model")));
        }
        std::fs::rename(part, target)?;
    }
    Ok(())
}

impl Model2Vec {
    pub fn load(dir: &Path) -> io::Result<Self> {
        let tokenizer: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("tokenizer.json"))?)
                .map_err(|e| invalid(format!("tokenizer.json: {e}")))?;
        let vocab: HashMap<String, u32> = tokenizer["model"]["vocab"]
            .as_object()
            .ok_or_else(|| invalid("tokenizer.json without a vocabulary"))?
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), u32::try_from(v.as_u64()?).ok()?)))
            .collect();
        let unk = tokenizer["model"]["unk_token"].as_str().and_then(|u| vocab.get(u).copied());
        let config: serde_json::Value = std::fs::read(dir.join("config.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let normalize = config["normalize"].as_bool().unwrap_or(true);

        let file = File::open(dir.join("model.safetensors"))?;
        // SAFETY: the model file is only read; it is not modified while mapped (it is
        // written only by `download`, via a renamed temporary file, before loading).
        #[allow(unsafe_code)]
        let map = unsafe { memmap2::Mmap::map(&file)? };
        let header_len = u64::from_le_bytes(
            map.get(..8)
                .ok_or_else(|| invalid("safetensors too short"))?
                .try_into()
                .map_err(|_| invalid("header"))?,
        );
        let data = usize::try_from(header_len)
            .ok()
            .and_then(|len| len.checked_add(8))
            .filter(|end| *end <= map.len())
            .ok_or_else(|| invalid("header cut short"))?;
        let header: serde_json::Value =
            serde_json::from_slice(&map[8..data]).map_err(|e| invalid(e.to_string()))?;
        let tensor = |name: &str| -> io::Result<(String, Vec<usize>, usize, usize)> {
            let t = &header[name];
            let dtype = t["dtype"]
                .as_str()
                .ok_or_else(|| invalid(format!("{name} is missing")))?
                .to_owned();
            let shape = t["shape"]
                .as_array()
                .map(|s| s.iter().filter_map(|d| d.as_u64().map(|d| d as usize)).collect())
                .unwrap_or_default();
            let offsets =
                t["data_offsets"].as_array().ok_or_else(|| invalid(format!("{name}: offsets")))?;
            let at = |i: usize| {
                offsets
                    .get(i)
                    .and_then(|o| o.as_u64())
                    .and_then(|o| usize::try_from(o).ok())
                    .and_then(|o| o.checked_add(data))
                    .ok_or_else(|| invalid(format!("{name}: offsets")))
            };
            let (start, end) = (at(0)?, at(1)?);
            Ok((dtype, shape, start, end))
        };
        let (dtype, shape, start, end) = tensor("embeddings")?;
        if dtype != "F32" || shape.len() != 2 || start > end || end > map.len() {
            return Err(invalid(format!("unexpected embeddings: {dtype} {shape:?}")));
        }
        let (rows, dims) = (shape[0], shape[1]);
        // Optional: the token → row mapping and the weights (quantized models).
        let mapping = match tensor("mapping") {
            Ok((t, _, s, e)) if t == "I64" && s <= e && e <= map.len() => {
                map[s..e].as_chunks::<8>().0.iter().map(|b| i64::from_le_bytes(*b) as u32).collect()
            }
            _ => (0..vocab.len() as u32).collect(),
        };
        let weights = match tensor("weights") {
            Ok((t, _, s, e)) if t == "F64" && s <= e && e <= map.len() => {
                map[s..e].as_chunks::<8>().0.iter().map(|b| f64::from_le_bytes(*b) as f32).collect()
            }
            Ok((t, _, s, e)) if t == "F32" && s <= e && e <= map.len() => {
                map[s..e].as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect()
            }
            _ => vec![1.0; vocab.len()],
        };
        tracing::info!(vocab = vocab.len(), rows, dims, "embeddings model loaded");
        Ok(Self { vocab, unk, mapping, weights, matrix: map, offset: start, dims, rows, normalize })
    }

    /// The WordPiece tokens of the text (lowercase, split on whitespace and punctuation,
    /// then longest match with `##` for continuations).
    pub fn tokenize(&self, text: &str) -> Vec<u32> {
        let mut ids = Vec::new();
        let lower = text.to_lowercase();
        let words = lower.split_whitespace().flat_map(|w| {
            // Punctuation is a separate token.
            let mut parts = Vec::new();
            let mut start = 0;
            for (i, c) in w.char_indices() {
                if c.is_ascii_punctuation() {
                    if start < i {
                        parts.push(&w[start..i]);
                    }
                    parts.push(&w[i..i + c.len_utf8()]);
                    start = i + c.len_utf8();
                }
            }
            if start < w.len() {
                parts.push(&w[start..]);
            }
            parts
        });
        for word in words {
            if ids.len() >= MAX_TOKENS {
                break;
            }
            if word.chars().count() > 100 {
                ids.extend(self.unk);
                continue;
            }
            let mut start = 0;
            let mut pieces = Vec::new();
            while start < word.len() {
                let mut end = word.len();
                let mut found = None;
                while end > start {
                    let piece = if start == 0 {
                        word[start..end].to_owned()
                    } else {
                        format!("##{}", &word[start..end])
                    };
                    if let Some(&id) = self.vocab.get(&piece) {
                        found = Some(id);
                        break;
                    }
                    end = word[..end].char_indices().last().map_or(start, |(i, _)| i);
                }
                match found {
                    Some(id) => {
                        pieces.push(id);
                        start = end;
                    }
                    None => {
                        pieces.clear();
                        pieces.extend(self.unk);
                        break;
                    }
                }
            }
            ids.extend(pieces);
        }
        ids
    }

    fn row(&self, row: usize, out: &mut [f32], weight: f32) {
        let start = self.offset + row * self.dims * 4;
        let Some(bytes) = self.matrix.get(start..start + self.dims * 4) else { return };
        for (o, b) in out.iter_mut().zip(bytes.as_chunks::<4>().0) {
            *o += f32::from_le_bytes(*b) * weight;
        }
    }
}

impl Embedder for Model2Vec {
    fn embed(&self, text: &str) -> Vec<f32> {
        let mut sum = vec![0.0f32; self.dims];
        let ids = self.tokenize(text);
        let mut count = 0usize;
        for id in ids {
            if Some(id) == self.unk {
                continue;
            }
            let Some(&row) = self.mapping.get(id as usize) else { continue };
            if (row as usize) < self.rows {
                let weight = self.weights.get(id as usize).copied().unwrap_or(1.0);
                self.row(row as usize, &mut sum, weight);
                count += 1;
            }
        }
        if count > 0 {
            sum.iter_mut().for_each(|x| *x /= count as f32);
        }
        if self.normalize {
            let norm = sum.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                sum.iter_mut().for_each(|x| *x /= norm);
            }
        }
        sum
    }

    fn dims(&self) -> usize {
        self.dims
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
