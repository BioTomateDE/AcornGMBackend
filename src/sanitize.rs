

pub fn sanitize_string(string: &str) -> Option<String> {
    let string = string.trim();   // trim whitespace
    
    if string.is_empty() {
        return None
    }
    
    // let banned_pattern: Regex = Regex::new(r"[\p{C}\\/]").unwrap(); // Control chars, backslashes
    // if banned_pattern.is_match(string) {
    //     return None
    // }
    
    // Normalize fancy quotes/dashes
    let normalized = string
        .replace('“', "\"")
        .replace('”', "\"")
        .replace('–', "-");

    Some(normalized)
}

