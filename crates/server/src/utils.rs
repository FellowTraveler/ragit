use base64::Engine;
use ragit_fs::{FileError, join4};

// TODO: don't unwrap this: return 500 if the user name or the repo name contains an invalid character
// ROOT/{user}/{repo}/.ragit
pub fn get_rag_path(user: &str, repo: &str) -> Result<String, FileError> {
    join4(
        "./data",  // TODO: make it configurable
        user,
        repo,
        ".ragit",
    )
}

pub fn decode_base64(s: &str) -> Result<Vec<u8>, ()> {
    base64::prelude::BASE64_STANDARD.decode(s).map_err(|_| ())
}

pub fn trim_long_string(s: &str, prefix_len: usize, suffix_len: usize) -> String {
    if s.len() <= (prefix_len + suffix_len) || s.chars().count() <= (prefix_len + suffix_len) {
        s.to_string()
    }

    else {
        format!(
            "{}...{}",
            s.chars().take(prefix_len).collect::<String>(),
            s.chars().rev().take(suffix_len).collect::<String>().chars().rev().collect::<String>(),
        )
    }
}
