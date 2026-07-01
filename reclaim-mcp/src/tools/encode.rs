//! Encoding + hashing tools. Pure functions on inline strings.
//! `encode`/`decode` schemes: base64 | base64url (no padding) | hex | url.
//! `hash` algos: md5 | sha1 | sha256 | sha512 | crc32.

use crate::server::ReclaimServer;
use base64::Engine;
use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router, Json};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub fn encode(input: &str, scheme: &str) -> Result<String, String> {
    let bytes = input.as_bytes();
    Ok(match scheme {
        "base64" => base64::engine::general_purpose::STANDARD.encode(bytes),
        "base64url" => base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes),
        "hex" => hex::encode(bytes),
        "url" => utf8_percent_encode(input, NON_ALPHANUMERIC).to_string(),
        other => return Err(format!("unknown scheme: {other}")),
    })
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DecodeOut {
    /// Decoded bytes as UTF-8, omitted when the bytes are not valid UTF-8.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Decoded bytes as lowercase hex (always present).
    pub hex: String,
}

pub fn decode(input: &str, scheme: &str) -> Result<DecodeOut, String> {
    let bytes: Vec<u8> = match scheme {
        "base64" => base64::engine::general_purpose::STANDARD
            .decode(input)
            .map_err(|e| e.to_string())?,
        "base64url" => base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(input)
            .map_err(|e| e.to_string())?,
        "hex" => hex::decode(input).map_err(|e| e.to_string())?,
        "url" => percent_decode_str(input).collect(),
        other => return Err(format!("unknown scheme: {other}")),
    };
    Ok(DecodeOut {
        text: String::from_utf8(bytes.clone()).ok(),
        hex: hex::encode(&bytes),
    })
}

pub fn hash(input: &str, algo: &str) -> Result<String, String> {
    let bytes = input.as_bytes();
    Ok(match algo {
        "md5" => {
            use md5::{Digest, Md5};
            hex::encode(Md5::digest(bytes))
        }
        "sha1" => {
            use sha1::{Digest, Sha1};
            hex::encode(Sha1::digest(bytes))
        }
        "sha256" => {
            use sha2::{Digest, Sha256};
            hex::encode(Sha256::digest(bytes))
        }
        "sha512" => {
            use sha2::{Digest, Sha512};
            hex::encode(Sha512::digest(bytes))
        }
        "crc32" => {
            let mut h = crc32fast::Hasher::new();
            h.update(bytes);
            format!("{:08x}", h.finalize())
        }
        other => return Err(format!("unknown algo: {other}")),
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EncodeReq {
    pub input: String,
    /// base64 | base64url | hex | url
    pub scheme: String,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct EncodeResp {
    pub output: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HashReq {
    pub input: String,
    /// md5 | sha1 | sha256 | sha512 | crc32
    pub algo: String,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct HashResp {
    pub algo: String,
    pub hex: String,
}

#[tool_router(router = encode_router, vis = "pub(crate)")]
impl ReclaimServer {
    #[tool(name = "encode", description = "Encode a string. scheme: base64 | base64url (no padding) | hex | url.")]
    pub async fn encode(&self, p: Parameters<EncodeReq>) -> Result<Json<EncodeResp>, String> {
        Ok(Json(EncodeResp { output: encode(&p.0.input, &p.0.scheme)? }))
    }

    #[tool(name = "decode", description = "Decode a string. scheme: base64 | base64url | hex | url. Returns hex always, plus text when the bytes are valid UTF-8.")]
    pub async fn decode(&self, p: Parameters<EncodeReq>) -> Result<Json<DecodeOut>, String> {
        Ok(Json(decode(&p.0.input, &p.0.scheme)?))
    }

    #[tool(name = "hash", description = "Hash a string. algo: md5 | sha1 | sha256 | sha512 | crc32. Returns lowercase hex.")]
    pub async fn hash(&self, p: Parameters<HashReq>) -> Result<Json<HashResp>, String> {
        let hex = hash(&p.0.input, &p.0.algo)?;
        Ok(Json(HashResp { algo: p.0.algo, hex }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        let e = encode("hello", "base64").unwrap();
        assert_eq!(e, "aGVsbG8=");
        let d = decode(&e, "base64").unwrap();
        assert_eq!(d.text.as_deref(), Some("hello"));
        assert_eq!(d.hex, "68656c6c6f");
    }

    #[test]
    fn base64url_has_no_padding_and_url_alphabet() {
        let e = encode("<<???>>", "base64url").unwrap();
        assert!(!e.contains('='));
        assert!(!e.contains('+') && !e.contains('/'));
        let d = decode(&e, "base64url").unwrap();
        assert_eq!(d.text.as_deref(), Some("<<???>>"));
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(encode("AB", "hex").unwrap(), "4142");
        assert_eq!(decode("4142", "hex").unwrap().text.as_deref(), Some("AB"));
    }

    #[test]
    fn url_encodes_reserved_chars() {
        assert_eq!(encode("a b&c", "url").unwrap(), "a%20b%26c");
        assert_eq!(decode("a%20b%26c", "url").unwrap().text.as_deref(), Some("a b&c"));
    }

    #[test]
    fn decode_of_non_utf8_omits_text_but_keeps_hex() {
        // 0xFF is not valid UTF-8.
        let d = decode("/w==", "base64").unwrap();
        assert!(d.text.is_none());
        assert_eq!(d.hex, "ff");
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(decode("zzz!", "hex").is_err());
        assert!(encode("x", "rot13").is_err());
    }

    #[test]
    fn hashes_match_known_vectors() {
        assert_eq!(hash("abc", "md5").unwrap(), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hash("abc", "sha1").unwrap(), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            hash("abc", "sha256").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hash("", "crc32").unwrap(), "00000000");
        assert_eq!(hash("abc", "crc32").unwrap(), "352441c2");
    }

    #[test]
    fn encode_router_lists_three_tools() {
        let names: Vec<String> = ReclaimServer::encode_router()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        for n in ["encode", "decode", "hash"] {
            assert!(names.contains(&n.to_string()), "missing {n}");
        }
    }
}
