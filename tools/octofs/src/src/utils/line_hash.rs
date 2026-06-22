


















use std::collections::HashMap;
use std::sync::OnceLock;


#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineMode {
	Number,
	Hash,
}

static LINE_MODE: OnceLock<LineMode> = OnceLock::new();


pub fn set_line_mode(mode: LineMode) {
	LINE_MODE.set(mode).ok();
}


pub fn get_line_mode() -> LineMode {
	LINE_MODE.get().copied().unwrap_or(LineMode::Number)
}


pub fn is_hash_mode() -> bool {
	get_line_mode() == LineMode::Hash
}



fn fnv1a_16(content: &str) -> u16 {
	const FNV_OFFSET: u32 = 2166136261;
	const FNV_PRIME: u32 = 16777619;

	let mut hash = FNV_OFFSET;
	for byte in content.bytes() {
		hash ^= byte as u32;
		hash = hash.wrapping_mul(FNV_PRIME);
	}


	((hash >> 16) ^ (hash & 0xFFFF)) as u16
}





pub fn compute_line_hashes(lines: &[&str]) -> Vec<String> {
	lines
		.iter()
		.enumerate()
		.map(|(i, line)| {
			let key = format!("{}:{}", i + 1, line);
			format!("{:04x}", fnv1a_16(&key))
		})
		.collect()
}


pub fn build_hash_to_line_map(lines: &[&str]) -> HashMap<String, usize> {
	let hashes = compute_line_hashes(lines);
	let mut map = HashMap::with_capacity(hashes.len());
	for (i, hash) in hashes.into_iter().enumerate() {
		map.insert(hash, i + 1);
	}
	map
}


pub fn resolve_hash_to_line(hash: &str, lines: &[&str]) -> Result<usize, String> {
	let map = build_hash_to_line_map(lines);
	map.get(hash)
		.copied()
		.ok_or_else(|| format!("Hash '{}' not found in file content", hash))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_basic_hash_deterministic() {

		let h1 = fnv1a_16("1:hello world");
		let h2 = fnv1a_16("1:hello world");
		assert_eq!(h1, h2);
	}

	#[test]
	fn test_different_content_different_hash() {
		let h1 = fnv1a_16("1:line one");
		let h2 = fnv1a_16("1:line two");
		assert_ne!(h1, h2);
	}

	#[test]
	fn test_compute_unique_lines() {
		let lines = vec!["fn main() {", "    println!(\"hello\");", "}"];
		let hashes = compute_line_hashes(&lines);
		assert_eq!(hashes.len(), 3);

		let unique: std::collections::HashSet<&String> = hashes.iter().collect();
		assert_eq!(unique.len(), 3);

		for h in &hashes {
			assert_eq!(h.len(), 4);
			assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
		}
	}

	#[test]
	fn test_duplicate_content_unique_hashes() {


		let lines = vec!["same", "same", "same"];
		let hashes = compute_line_hashes(&lines);
		assert_eq!(hashes.len(), 3);
		let unique: std::collections::HashSet<&String> = hashes.iter().collect();
		assert_eq!(
			unique.len(),
			3,
			"duplicate lines must get unique hashes via position"
		);

		assert_eq!(hashes[0], format!("{:04x}", fnv1a_16("1:same")));
		assert_eq!(hashes[1], format!("{:04x}", fnv1a_16("2:same")));
		assert_eq!(hashes[2], format!("{:04x}", fnv1a_16("3:same")));
	}

	#[test]
	fn test_same_content_different_position_different_hash() {

		let h1 = fnv1a_16("1:closing brace");
		let h2 = fnv1a_16("5:closing brace");
		assert_ne!(h1, h2);
	}

	#[test]
	fn test_reverse_lookup() {
		let lines = vec!["first", "second", "third"];
		let map = build_hash_to_line_map(&lines);
		assert_eq!(map.len(), 3);

		let hashes = compute_line_hashes(&lines);
		assert_eq!(map[&hashes[0]], 1);
		assert_eq!(map[&hashes[1]], 2);
		assert_eq!(map[&hashes[2]], 3);
	}

	#[test]
	fn test_resolve_hash() {
		let lines = vec!["first", "second", "third"];
		let hashes = compute_line_hashes(&lines);

		assert_eq!(resolve_hash_to_line(&hashes[0], &lines).unwrap(), 1);
		assert_eq!(resolve_hash_to_line(&hashes[1], &lines).unwrap(), 2);
		assert_eq!(resolve_hash_to_line(&hashes[2], &lines).unwrap(), 3);
		assert!(resolve_hash_to_line("zzzz", &lines).is_err());
	}

	#[test]
	fn test_empty_lines() {

		let lines = vec!["", "", "content"];
		let hashes = compute_line_hashes(&lines);
		assert_eq!(hashes.len(), 3);
		let unique: std::collections::HashSet<&String> = hashes.iter().collect();
		assert_eq!(unique.len(), 3);
	}

	#[test]
	fn test_hash_format_lowercase_hex() {
		let lines = vec!["test"];
		let hashes = compute_line_hashes(&lines);
		assert!(hashes[0]
			.chars()
			.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
	}

	#[test]
	fn test_unique_content_hash_stable_at_same_position() {

		let lines_a = vec!["alpha", "beta", "gamma"];
		let lines_b = vec!["alpha", "beta", "gamma"];
		let hashes_a = compute_line_hashes(&lines_a);
		let hashes_b = compute_line_hashes(&lines_b);
		assert_eq!(hashes_a, hashes_b);
	}

	#[test]
	fn test_modified_line_changes_hash() {
		let lines_before = vec!["alpha", "beta", "gamma"];
		let hashes_before = compute_line_hashes(&lines_before);

		let lines_after = vec!["alpha", "BETA_MODIFIED", "gamma"];
		let hashes_after = compute_line_hashes(&lines_after);

		assert_eq!(hashes_before[0], hashes_after[0]);
		assert_ne!(hashes_before[1], hashes_after[1]);
		assert_eq!(hashes_before[2], hashes_after[2]);
	}
}
