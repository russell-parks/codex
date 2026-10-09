use sha2::Digest;

pub fn maybe_hash_prompt(hash_prompts: bool, prompt: &str) -> Option<String> {
    if !hash_prompts {
        return None;
    }
    Some(format!("{:x}", sha2::Sha256::digest(prompt.as_bytes())))
}

pub fn maybe_store_prompt(log_user_prompt: bool, prompt: &str) -> Option<String> {
    log_user_prompt.then(|| prompt.to_string())
}
