//! Compile-time string obfuscation for on-prem / client-server builds.
//!
//! Sensitive string literals (agent prompts, safety policies, internal
//! instructions) otherwise sit in the shipped binary verbatim, so a single
//! `strings ./api` on a customer-controlled server reveals them. With the
//! `obfuscate` feature enabled, [`obf!`] stores the literal XOR-encoded and
//! decodes it once at runtime, so the plaintext never appears in the binary.
//!
//! Without the feature `obf!("x")` is exactly `String::from("x")`: identical
//! behaviour and no build-time cost, so the default (our own hosting) build is
//! unchanged. Obfuscation is enabled only for the client-facing release
//! profile — see `deploy/lite/Dockerfile.release`.
//!
//! This is deliberately a light layer: it defeats casual inspection and
//! `strings` dumps, not a determined reverse engineer with a debugger on a box
//! they control. It does not (and cannot) protect secrets that live in
//! `Config.toml` on the customer's server.

#[cfg(feature = "obfuscate")]
pub use imp::{decode, encode};

/// FNV-1a content digest, computable in const context.
///
/// Used to fingerprint source files at compile time (see [`src_digest!`]) so an
/// obfuscated build can embed a change-sensitive revision marker without
/// shipping the source text itself. Not a cryptographic hash — collision
/// resistance is not required for revision detection.
#[must_use]
#[allow(clippy::cast_lossless)]
pub const fn digest_u64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    hash
}

/// Compile-time content digest of a file, as a `u64`.
///
/// `src_digest!("path")` hashes the file's bytes during const evaluation, so
/// only the digest (not the source) is emitted into the binary. Use it in place
/// of `include_str!` wherever a file is embedded solely to be hashed.
#[macro_export]
macro_rules! src_digest {
    ($path:literal) => {{
        const __SRC_DIGEST: u64 = $crate::obfs::digest_u64(include_str!($path).as_bytes());
        __SRC_DIGEST
    }};
}

#[cfg(feature = "obfuscate")]
mod imp {
    use super::digest_u64;

    /// Compile-time keystream seed. Overridable per build via
    /// `TZ_OBFUSCATION_KEY` so every customer deployment ships a different
    /// keystream; falls back to a fixed seed when unset.
    const SEED: u64 = match option_env!("TZ_OBFUSCATION_KEY") {
        Some(key) => digest_u64(key.as_bytes()),
        None => 0x9E37_79B9_7F4A_7C15,
    };

    /// Position-dependent keystream byte (xorshift* mixed with the index) so
    /// repeated characters do not encode to a repeated cipher byte.
    #[allow(clippy::cast_possible_truncation)]
    const fn keystream(index: usize) -> u8 {
        let mut x = SEED
            ^ (index as u64)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(0x1234_5678_9ABC_DEF0);
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
    }

    /// Encode a literal at compile time. The plaintext is consumed during
    /// const evaluation and never emitted into the binary; only the returned
    /// cipher array is.
    #[allow(clippy::cast_possible_truncation)]
    pub const fn encode<const N: usize>(src: &[u8]) -> [u8; N] {
        let mut out = [0u8; N];
        let mut i = 0;
        while i < N {
            out[i] = src[i] ^ keystream(i);
            i += 1;
        }
        out
    }

    /// Decode a cipher array back to the original string at runtime.
    #[must_use]
    pub fn decode(cipher: &[u8]) -> String {
        let bytes: Vec<u8> = cipher
            .iter()
            .enumerate()
            .map(|(i, byte)| byte ^ keystream(i))
            .collect();
        // The input is always a valid UTF-8 literal encoded by `encode`.
        String::from_utf8(bytes).unwrap_or_default()
    }
}

/// Obfuscate a string literal.
///
/// Returns an owned `String`. With the `obfuscate` feature the literal is
/// XOR-encoded into the binary and decoded at runtime; without it, this is a
/// plain `String::from` of the literal.
#[cfg(feature = "obfuscate")]
#[macro_export]
macro_rules! obf {
    ($s:literal) => {{
        const __OBF_SRC: &[u8] = $s.as_bytes();
        const __OBF_LEN: usize = __OBF_SRC.len();
        const __OBF_CIPHER: [u8; __OBF_LEN] = $crate::obfs::encode::<__OBF_LEN>(__OBF_SRC);
        $crate::obfs::decode(&__OBF_CIPHER)
    }};
}

/// Obfuscate a string literal (no-op passthrough when the `obfuscate` feature
/// is disabled).
#[cfg(not(feature = "obfuscate"))]
#[macro_export]
macro_rules! obf {
    ($s:literal) => {{ ::std::string::String::from($s) }};
}

#[cfg(test)]
mod tests {
    #[test]
    fn roundtrips_ascii_and_unicode() {
        assert_eq!(crate::obf!("hello world"), "hello world");
        assert_eq!(
            crate::obf!("Ты — виртуальный ассистент"),
            "Ты — виртуальный ассистент",
        );
    }

    #[test]
    fn roundtrips_multiline_raw() {
        let expected = "<tag>\nline one\nline two\n</tag>";
        assert_eq!(crate::obf!("<tag>\nline one\nline two\n</tag>"), expected);
    }
}
