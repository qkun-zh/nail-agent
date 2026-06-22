















use super::super::McpToolCall;
use super::core::save_file_history;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::fs as tokio_fs;
use tokio::sync::Mutex as AsyncMutex;




static FILE_LOCKS: OnceLock<Mutex<HashMap<String, Arc<AsyncMutex<()>>>>> = OnceLock::new();

fn get_file_locks() -> &'static Mutex<HashMap<String, Arc<AsyncMutex<()>>>> {
	FILE_LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

// Build the lock-map key for a path. Canonicalize when possible so aliases
// (`x`, `./x`, `/abs/x`, symlinks) map to the same lock — otherwise two
// requests targeting the same file would not serialize and could corrupt it.
// Falls back to the raw path string if canonicalize fails (file may not exist
// yet, e.g. for `text_editor create`).
pub fn lock_key_for(path: &Path) -> String {
	if let Ok(canon) = path.canonicalize() {
		return canon.to_string_lossy().to_string();
	}
	// File may not exist yet (create) or has been removed (delete+undo).
	// Canonicalize the parent and append the file name so the key stays
	// consistent across the file's lifetime.
	if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
		if let Ok(canon_parent) = parent.canonicalize() {
			return canon_parent.join(name).to_string_lossy().to_string();
		}
	}
	path.to_string_lossy().to_string()
}


async fn acquire_file_lock(path: &Path) -> Result<Arc<AsyncMutex<()>>> {
	let key = lock_key_for(path);

	let file_lock = {
		let mut locks = get_file_locks().lock().expect("file locks poisoned");
		locks
			.entry(key)
			.or_insert_with(|| Arc::new(AsyncMutex::new(())))
			.clone()
	};

	Ok(file_lock)
}



pub async fn delete_file_spec(path: &Path) -> Result<String> {
	if !path.exists() {
		bail!("File does not exist: {}", path.display());
	}
	let meta = tokio_fs::metadata(path)
		.await
		.map_err(|e| anyhow!("Failed to stat '{}': {}", path.display(), e))?;
	if meta.is_dir() {
		bail!(
			"Path is a directory, not a file: {}. Use the shell tool with `rm -r` to remove directories.",
			path.display()
		);
	}

	let file_lock = acquire_file_lock(path).await?;
	let _lock_guard = file_lock.lock().await;


	save_file_history(path).await?;

	tokio_fs::remove_file(path)
		.await
		.map_err(|e| anyhow!("Failed to delete '{}': {}", path.display(), e))?;

	Ok(format!("Successfully deleted {}", path.display()))
}



fn resolve_line_index(index: i64, total_lines: usize) -> Result<usize, String> {
	if index == 0 {
		return Err("Line numbers are 1-indexed, use 1 for first line".to_string());
	}

	if index > 0 {
		let pos_index = index as usize;
		if pos_index > total_lines {
			return Err(format!(
				"Line {} exceeds file length ({} lines)",
				index, total_lines
			));
		}
		Ok(pos_index)
	} else {

		let from_end = (-index) as usize;
		if from_end > total_lines {
			return Err(format!(
				"Negative index {} exceeds file length ({} lines)",
				index, total_lines
			));
		}
		Ok(total_lines - from_end + 1)
	}
}


fn resolve_line_range_batch(
	start: i64,
	end: i64,
	total_lines: usize,
) -> Result<(usize, usize), String> {
	let resolved_start = resolve_line_index(start, total_lines)?;
	let resolved_end = resolve_line_index(end, total_lines)?;

	if resolved_start > resolved_end {
		return Err(format!(
			"Start line ({}) cannot be greater than end line ({})",
			start, end
		));
	}

	Ok((resolved_start, resolved_end))
}


#[derive(Debug, Clone)]
struct BatchOperation {
	operation_type: OperationType,
	line_range: LineRange,
	content: String,
	operation_index: usize,
}


#[derive(Debug, Clone)]
struct UnresolvedBatchOperation {
	operation_type: OperationType,
	line_range: UnresolvedLineRange,
	content: String,
	operation_index: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum OperationType {
	Insert,
	Replace,
}

#[derive(Debug, Clone)]
enum LineRange {
	Single(usize),
	Range(usize, usize),
}

#[derive(Debug, Clone)]
enum UnresolvedLineRange {
	Single(i64),
	Range(i64, i64),
	Hash(String),
	HashRange(String, String),
}



fn resolve_unresolved_line_range(
	unresolved: &UnresolvedLineRange,
	total_lines: usize,
	lines: &[&str],
) -> Result<LineRange, String> {
	match unresolved {
		UnresolvedLineRange::Single(line) => {

			if *line == 0 {
				return Ok(LineRange::Single(0));
			}
			let resolved = resolve_line_index(*line, total_lines)?;
			Ok(LineRange::Single(resolved))
		}
		UnresolvedLineRange::Range(start, end) => {
			let (resolved_start, resolved_end) =
				resolve_line_range_batch(*start, *end, total_lines)?;
			Ok(LineRange::Range(resolved_start, resolved_end))
		}
		UnresolvedLineRange::Hash(hash) => {
			let line = crate::utils::line_hash::resolve_hash_to_line(hash, lines)?;
			Ok(LineRange::Single(line))
		}
		UnresolvedLineRange::HashRange(start_hash, end_hash) => {
			let start = crate::utils::line_hash::resolve_hash_to_line(start_hash, lines)?;
			let end = crate::utils::line_hash::resolve_hash_to_line(end_hash, lines)?;
			if start > end {
				return Err(format!(
					"Hash range is reversed: '{}' is line {} but '{}' is line {} (which comes before it). \
					Did you mean line_range: [\"{}\", \"{}\"]?",
					start_hash, start, end_hash, end, end_hash, start_hash
				));
			}
			Ok(LineRange::Range(start, end))
		}
	}
}



fn normalize_whitespace(s: &str) -> String {
	s.lines()
		.map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
		.collect::<Vec<_>>()
		.join("\n")
}


fn find_all_positions(haystack: &str, needle: &str) -> Vec<usize> {
	let mut positions = Vec::new();
	let mut start = 0;
	while let Some(pos) = haystack[start..].find(needle) {
		positions.push(start + pos);
		start += pos + needle.len();
	}
	positions
}


fn byte_offset_to_line(content: &str, offset: usize) -> usize {
	content[..offset].matches('\n').count() + 1
}



fn similarity_ratio(a: &str, b: &str) -> f64 {
	let a_lines: Vec<&str> = a.lines().collect();
	let b_lines: Vec<&str> = b.lines().collect();
	if a_lines.is_empty() && b_lines.is_empty() {
		return 1.0;
	}
	let max_lines = a_lines.len().max(b_lines.len());
	let mut total = 0.0;
	for i in 0..max_lines {
		let la = a_lines.get(i).unwrap_or(&"");
		let lb = b_lines.get(i).unwrap_or(&"");
		total += line_similarity(la, lb);
	}
	total / max_lines as f64
}


fn line_similarity(a: &str, b: &str) -> f64 {
	let a_chars: Vec<char> = a.chars().collect();
	let b_chars: Vec<char> = b.chars().collect();
	let total = a_chars.len() + b_chars.len();
	if total == 0 {
		return 1.0;
	}
	let lcs_len = lcs_length(&a_chars, &b_chars);
	(2.0 * lcs_len as f64) / total as f64
}


fn lcs_length(a: &[char], b: &[char]) -> usize {

	const MAX_CHARS: usize = 2000;
	let a = if a.len() > MAX_CHARS {
		&a[..MAX_CHARS]
	} else {
		a
	};
	let b = if b.len() > MAX_CHARS {
		&b[..MAX_CHARS]
	} else {
		b
	};

	let mut prev = vec![0usize; b.len() + 1];
	let mut curr = vec![0usize; b.len() + 1];
	for &ac in a {
		for (j, &bc) in b.iter().enumerate() {
			curr[j + 1] = if ac == bc {
				prev[j] + 1
			} else {
				prev[j + 1].max(curr[j])
			};
		}
		std::mem::swap(&mut prev, &mut curr);
		curr.iter_mut().for_each(|v| *v = 0);
	}
	*prev.last().unwrap_or(&0)
}


fn diagnose_mismatch(expected: &str, actual: &str) -> String {
	let exp_norm = normalize_whitespace(expected);
	let act_norm = normalize_whitespace(actual);
	if exp_norm == act_norm {
		return "whitespace/indentation mismatch only".to_string();
	}

	let exp_trimmed: Vec<&str> = expected.lines().map(|l| l.trim()).collect();
	let act_trimmed: Vec<&str> = actual.lines().map(|l| l.trim()).collect();
	if exp_trimmed == act_trimmed {
		return "indentation mismatch only".to_string();
	}
	"content differs".to_string()
}



fn find_closest_matches(content: &str, needle: &str, top_n: usize) -> Vec<(usize, String, f64)> {
	let content_lines: Vec<&str> = content.lines().collect();
	let needle_lines: Vec<&str> = needle.lines().collect();
	let needle_count = needle_lines.len().max(1);

	if content_lines.len() < needle_count {
		return Vec::new();
	}

	let mut candidates: Vec<(usize, String, f64)> = Vec::new();

	for start in 0..=(content_lines.len() - needle_count) {
		let window: String = content_lines[start..start + needle_count].join("\n");
		let sim = similarity_ratio(needle, &window);

		if sim >= 0.4 {
			candidates.push((start + 1, window, sim));
		}
	}


	candidates.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
	candidates.truncate(top_n);
	candidates
}


fn detect_indent(text: &str) -> &str {
	for line in text.lines() {
		if !line.trim().is_empty() {
			let trimmed = line.trim_start();
			return &line[..line.len() - trimmed.len()];
		}
	}
	""
}




fn adjust_indentation(new_text: &str, provided_old: &str, actual_old: &str) -> String {
	let provided_indent = detect_indent(provided_old);
	let actual_indent = detect_indent(actual_old);

	if provided_indent == actual_indent {
		return new_text.to_string();
	}


	let provided_len = provided_indent.len();

	new_text
		.lines()
		.map(|line| {
			if line.trim().is_empty() {
				return line.to_string();
			}

			if provided_len > 0 && line.starts_with(provided_indent) {
				format!("{}{}", actual_indent, &line[provided_len..])
			} else {

				let line_indent_len = line.len() - line.trim_start().len();
				if line_indent_len >= provided_len {

					format!("{}{}", actual_indent, &line[provided_len..])
				} else {

					format!("{}{}", actual_indent, line.trim_start())
				}
			}
		})
		.collect::<Vec<_>>()
		.join("\n")
}




fn build_str_replace_diff(
	orig_lines: &[&str],
	new_lines: &[&str],
	start: usize,
	old_line_count: usize,
	new_text_lines: &[&str],
) -> String {
	const CONTEXT: usize = 2;
	let mut diff: Vec<String> = Vec::new();


	let ctx_before_start = start.saturating_sub(CONTEXT);
	if ctx_before_start > 0 {
		diff.push("...".to_string());
	}
	for (i, line) in orig_lines
		.iter()
		.enumerate()
		.take(start)
		.skip(ctx_before_start)
	{
		diff.push(format!("{}: {}", i + 1, line));
	}


	for (i, line) in orig_lines
		.iter()
		.enumerate()
		.skip(start)
		.take(old_line_count)
	{
		diff.push(format!("-{}: {}", i + 1, line));
	}



	let new_block_start = start + 1;
	for (i, line) in new_text_lines.iter().enumerate() {
		diff.push(format!("+{}: {}", new_block_start + i, line));
	}


	let new_after_start = start + new_text_lines.len();
	let ctx_after_end = (new_after_start + CONTEXT).min(new_lines.len());
	for (i, line) in new_lines
		.iter()
		.enumerate()
		.take(ctx_after_end)
		.skip(new_after_start)
	{
		diff.push(format!("{}: {}", i + 1, line));
	}
	if ctx_after_end < new_lines.len() {
		diff.push("...".to_string());
	}

	diff.join("\n")
}





pub async fn atomic_write(path: &Path, content: &str) -> Result<()> {
	let parent_dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
	let tmp_path = parent_dir.join(format!(
		".octofs_tmp_{}.tmp",
		path.file_name().unwrap_or_default().to_string_lossy()
	));



	let original_perms = match tokio_fs::metadata(path).await {
		Ok(meta) => Some(meta.permissions()),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
		Err(e) => {
			return Err(anyhow!(
				"Failed to read metadata for '{}': {}",
				path.display(),
				e
			))
		}
	};

	tokio_fs::write(&tmp_path, content)
		.await
		.map_err(|e| anyhow!("Failed to write temp file for '{}': {}", path.display(), e))?;

	if let Some(perms) = original_perms {
		if let Err(e) = tokio_fs::set_permissions(&tmp_path, perms).await {
			let _ = tokio_fs::remove_file(&tmp_path).await;
			return Err(anyhow!(
				"Failed to preserve permissions on '{}': {}",
				path.display(),
				e
			));
		}
	}

	if let Err(e) = tokio_fs::rename(&tmp_path, path).await {

		let _ = tokio_fs::remove_file(&tmp_path).await;
		return Err(anyhow!(
			"Failed to atomically replace '{}': {}",
			path.display(),
			e
		));
	}
	Ok(())
}




pub async fn str_replace_spec(path: &Path, old_text: &str, new_text: &str) -> Result<String> {
	if !path.exists() {
		bail!("File not found");
	}


	let file_lock = acquire_file_lock(path).await?;
	let _lock_guard = file_lock.lock().await;


	let content = tokio_fs::read_to_string(path)
		.await
		.map_err(|e| anyhow!("Permission denied. Cannot read file: {}", e))?;


	let occurrences = content.matches(old_text).count();

	if occurrences == 1 {

		let orig_lines: Vec<&str> = content.lines().collect();
		let old_line_count = old_text.lines().count();

		let match_offset = content.find(old_text).unwrap_or(0);
		let match_start = byte_offset_to_line(&content, match_offset) - 1;

		save_file_history(path).await?;
		let new_content = content.replace(old_text, new_text);
		atomic_write(path, &new_content).await?;

		if old_line_count > 1 {
			crate::mcp::hint_accumulator::push_hint(&format!(
				"`str_replace` matched {} lines. Prefer `batch_edit` when you know the line range — it's faster and avoids content-search ambiguity.",
				old_line_count
			));
		}

		let new_lines: Vec<&str> = new_content.lines().collect();
		let new_text_lines: Vec<&str> = new_text.lines().collect();
		let diff = build_str_replace_diff(
			&orig_lines,
			&new_lines,
			match_start,
			old_line_count,
			&new_text_lines,
		);
		return Ok(diff);
	}

	if occurrences > 1 {

		let positions = find_all_positions(&content, old_text);
		let use_hashes = crate::utils::line_hash::is_hash_mode();
		let file_lines: Vec<&str> = content.lines().collect();
		let hashes: Vec<String> = if use_hashes {
			crate::utils::line_hash::compute_line_hashes(&file_lines)
		} else {
			Vec::new()
		};
		let locations: Vec<String> = positions
			.iter()
			.enumerate()
			.map(|(i, &offset)| {
				let line = byte_offset_to_line(&content, offset);
				if use_hashes {
					format!("  {}. hash {} (line {})", i + 1, hashes[line - 1], line)
				} else {
					format!("  {}. line {}", i + 1, line)
				}
			})
			.collect();

		bail!(
			"Found {} matches for replacement text at:\n{}\nAdd more surrounding context to make a unique match, or use `batch_edit` with the specific {}.",
			occurrences,
			locations.join("\n"),
			if use_hashes { "hash range" } else { "line range" }
		);
	}


	let norm_old = normalize_whitespace(old_text);
	let norm_content = normalize_whitespace(&content);
	let norm_occurrences = norm_content.matches(&norm_old).count();

	if norm_occurrences == 1 {


		let content_lines: Vec<&str> = content.lines().collect();
		let old_lines: Vec<&str> = old_text.lines().collect();
		let old_line_count = old_lines.len();

		let mut match_start = None;
		for start in 0..=content_lines.len().saturating_sub(old_line_count) {
			let window: Vec<&str> = content_lines[start..start + old_line_count].to_vec();
			let window_norm: Vec<String> = window
				.iter()
				.map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
				.collect();
			let old_norm: Vec<String> = old_lines
				.iter()
				.map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
				.collect();
			if window_norm == old_norm {
				match_start = Some(start);
				break;
			}
		}

		if let Some(start) = match_start {
			let actual_old = content_lines[start..start + old_line_count].join("\n");
			let adjusted_new = adjust_indentation(new_text, old_text, &actual_old);

			save_file_history(path).await?;
			let new_content = content.replace(&actual_old, &adjusted_new);
			atomic_write(path, &new_content).await?;

			crate::mcp::hint_accumulator::push_hint(
				"Replaced via fuzzy match (whitespace-normalized). Indentation was auto-adjusted to match the file.",
			);

			let new_lines: Vec<&str> = new_content.lines().collect();
			let new_text_lines: Vec<&str> = adjusted_new.lines().collect();
			let diff = build_str_replace_diff(
				&content_lines,
				&new_lines,
				start,
				old_line_count,
				&new_text_lines,
			);
			return Ok(diff);
		}
	}


	let candidates = find_closest_matches(&content, old_text, 3);

	let mut msg = String::from(
		"No exact match found. Make sure you pass raw content (no escaped \\t, \\n).\n",
	);

	if candidates.is_empty() {
		msg.push_str("No similar text found in the file. Verify the content exists.");
	} else {
		let use_hashes = crate::utils::line_hash::is_hash_mode();
		let diag_lines: Vec<&str> = content.lines().collect();
		let diag_hashes: Vec<String> = if use_hashes {
			crate::utils::line_hash::compute_line_hashes(&diag_lines)
		} else {
			Vec::new()
		};

		msg.push_str("Closest matches:\n");
		let old_line_count = old_text.lines().count();
		for (i, (line_num, window, sim)) in candidates.iter().enumerate() {
			let diagnosis = diagnose_mismatch(old_text, window);
			let end_line = line_num + old_line_count - 1;
			if use_hashes {
				let start_hash = &diag_hashes[line_num - 1];
				let end_hash = &diag_hashes[end_line - 1];
				msg.push_str(&format!(
					"\n  {}. Hashes {}-{} ({:.0}% similar, {})\n",
					i + 1,
					start_hash,
					end_hash,
					sim * 100.0,
					diagnosis
				));
			} else {
				msg.push_str(&format!(
					"\n  {}. Lines {}-{} ({:.0}% similar, {})\n",
					i + 1,
					line_num,
					end_line,
					sim * 100.0,
					diagnosis
				));
			}

			for (j, line) in window.lines().take(3).enumerate() {
				let pfx = if use_hashes {
					diag_hashes[line_num - 1 + j].clone()
				} else {
					format!("{}", line_num + j)
				};
				msg.push_str(&format!("     {}: {}\n", pfx, line));
			}
			if old_line_count > 3 {
				msg.push_str(&format!("     ... ({} more lines)\n", old_line_count - 3));
			}
		}
		msg.push_str(&format!(
			"\nTip: use `batch_edit` with the {} shown above, or fix the `old_text` content.",
			if use_hashes {
				"hash range"
			} else {
				"line range"
			}
		));
	}

	bail!("{}", msg);
}








fn is_structural_noise(line: &str) -> bool {
	let trimmed = line.trim();

	if trimmed.is_empty() {
		return true;
	}

	let core = trimmed.trim_end_matches([',', ';']);

	matches!(core, "}" | "]" | ")")
}






fn check_replace_duplicates(
	content_lines: &[&str],
	file_lines: &[&str],
	start_line: usize,
	end_line: usize,
	operation_index: usize,
	hashes: &[String],
	use_hashes: bool,
) -> Result<(), String> {
	if content_lines.is_empty() {
		return Ok(());
	}


	let line_id = |line_1idx: usize| -> String {
		if use_hashes {
			hashes[line_1idx - 1].clone()
		} else {
			format!("line {}", line_1idx)
		}
	};
	let range_id = |s: usize, e: usize| -> String {
		if use_hashes {
			format!("[{},{}]", hashes[s - 1], hashes[e - 1])
		} else {
			format!("[{}-{}]", s, e)
		}
	};


	if start_line > 1 {
		let line_before = file_lines[start_line - 2];
		if content_lines[0] == line_before && !is_structural_noise(line_before) {
			return Err(format!(
				"Duplicate line detected in operation {}: content's first line matches {} \
				(just before the replacement range {}). \
				{}: {:?}. Do NOT include surrounding unchanged lines — \
				only provide the lines that replace {}.",
				operation_index,
				line_id(start_line - 1),
				range_id(start_line, end_line),
				line_id(start_line - 1),
				line_before,
				range_id(start_line, end_line)
			));
		}
	}

	if end_line < file_lines.len() {
		let line_after = file_lines[end_line];
		let last = content_lines[content_lines.len() - 1];
		if last == line_after && !is_structural_noise(line_after) {
			return Err(format!(
				"Duplicate line detected in operation {}: content's last line matches {} \
				(just after the replacement range {}). \
				{}: {:?}. Do NOT include surrounding unchanged lines — \
				only provide the lines that replace {}.",
				operation_index,
				line_id(end_line + 1),
				range_id(start_line, end_line),
				line_id(end_line + 1),
				line_after,
				range_id(start_line, end_line)
			));
		}
	}
	Ok(())
}








fn detect_conflicts(
	operations: &[BatchOperation],
	hashes: &[String],
	use_hashes: bool,
) -> Result<(), String> {
	let id = |line_1idx: usize| -> String {
		if use_hashes {
			hashes[line_1idx - 1].clone()
		} else {
			format!("{}", line_1idx)
		}
	};

	for i in 0..operations.len() {
		for j in (i + 1)..operations.len() {
			let op1 = &operations[i];
			let op2 = &operations[j];

			match (&op1.operation_type, &op2.operation_type) {

				(OperationType::Replace, OperationType::Replace) => {
					let (s1, e1) = replace_range(&op1.line_range);
					let (s2, e2) = replace_range(&op2.line_range);

					if s1 <= e2 && s2 <= e1 {
						return Err(format!(
							"Conflicting operations: operation {} (replace [{},{}]) and {} (replace [{},{}]) have overlapping ranges",
							op1.operation_index, id(s1), id(e1), op2.operation_index, id(s2), id(e2)
						));
					}
				}

				(OperationType::Insert, OperationType::Insert) => {
					let line1 = insert_anchor(&op1.line_range);
					let line2 = insert_anchor(&op2.line_range);
					if line1 == line2 {
						return Err(format!(
							"Conflicting operations: operation {} and {} both insert after {}",
							op1.operation_index,
							op2.operation_index,
							id(line1)
						));
					}
				}


				(OperationType::Insert, OperationType::Replace)
				| (OperationType::Replace, OperationType::Insert) => {}
			}
		}
	}
	Ok(())
}


fn replace_range(line_range: &LineRange) -> (usize, usize) {
	match line_range {
		LineRange::Range(start, end) => (*start, *end),
		LineRange::Single(line) => (*line, *line),
	}
}


fn insert_anchor(line_range: &LineRange) -> usize {
	match line_range {
		LineRange::Single(line) => *line,
		LineRange::Range(start, _) => *start,
	}
}








async fn apply_batch_operations(
	original_content: &str,
	operations: &[BatchOperation],
) -> Result<String> {
	let mut lines: Vec<String> = original_content.lines().map(|s| s.to_string()).collect();
	let original_len = lines.len();


	let mut replaces: Vec<&BatchOperation> = operations
		.iter()
		.filter(|op| op.operation_type == OperationType::Replace)
		.collect();
	let mut inserts: Vec<&BatchOperation> = operations
		.iter()
		.filter(|op| op.operation_type == OperationType::Insert)
		.collect();


	replaces.sort_by(|a, b| {
		let sa = match &a.line_range {
			LineRange::Range(s, _) => *s,
			LineRange::Single(l) => *l,
		};
		let sb = match &b.line_range {
			LineRange::Range(s, _) => *s,
			LineRange::Single(l) => *l,
		};
		sb.cmp(&sa)
	});



	let mut replace_deltas: Vec<(usize, usize, i64)> = Vec::with_capacity(replaces.len());

	for operation in &replaces {
		let (start, end) = match operation.line_range {
			LineRange::Range(start, end) => (start, end),
			LineRange::Single(line) => (line, line),
		};


		if start == 0 || end == 0 {
			return Err(anyhow!("Line numbers must be 1-indexed (start from 1)"));
		}
		if start > original_len || end > original_len {
			return Err(anyhow!(
				"Line range [{}, {}] is beyond file length {}",
				start,
				end,
				original_len
			));
		}
		if start > end {
			return Err(anyhow!("Invalid line range: start {} > end {}", start, end));
		}

		let old_count = end - start + 1;
		let content_lines: Vec<String> = operation.content.lines().map(|s| s.to_string()).collect();
		let new_count = content_lines.len();


		let start_idx = start - 1;
		for _ in 0..old_count {
			lines.remove(start_idx);
		}


		for (i, line) in content_lines.into_iter().enumerate() {
			lines.insert(start_idx + i, line);
		}

		replace_deltas.push((start, end, new_count as i64 - old_count as i64));
	}




	inserts.sort_by(|a, b| {
		let la = match &a.line_range {
			LineRange::Single(l) => *l,
			LineRange::Range(s, _) => *s,
		};
		let lb = match &b.line_range {
			LineRange::Single(l) => *l,
			LineRange::Range(s, _) => *s,
		};
		lb.cmp(&la)
	});

	for operation in &inserts {
		let original_anchor = match operation.line_range {
			LineRange::Single(line) => line,
			_ => return Err(anyhow!("Insert operation must use single line number")),
		};


		if original_anchor > original_len {
			return Err(anyhow!(
				"Insert position {} is beyond file length {}",
				original_anchor,
				original_len
			));
		}









		let mut adjusted = original_anchor as i64;
		for &(rs, re, delta) in &replace_deltas {
			if original_anchor >= re {

				adjusted += delta;
			} else if original_anchor >= rs {



				let old_count = (re - rs + 1) as i64;
				let new_count = old_count + delta;
				let offset_in_old = (original_anchor - rs) as i64;

				let offset_in_new = offset_in_old.min(new_count);




				adjusted += (rs as i64 + offset_in_new) - original_anchor as i64;
			}

		}

		let insert_pos = adjusted.max(0) as usize;


		let content_lines: Vec<String> = operation.content.lines().map(|s| s.to_string()).collect();

		if insert_pos == 0 {
			for (i, line) in content_lines.into_iter().enumerate() {
				lines.insert(i, line);
			}
		} else {
			let clamped = insert_pos.min(lines.len());
			for (i, line) in content_lines.into_iter().enumerate() {
				lines.insert(clamped + i, line);
			}
		}
	}


	let result = lines.join("\n");
	if original_content.ends_with('\n') && !result.ends_with('\n') {
		Ok(format!("{}\n", result))
	} else {
		Ok(result)
	}
}


fn parse_line_range(
	value: &Value,
	operation_type: &OperationType,
) -> Result<UnresolvedLineRange, String> {
	match value {
		Value::Number(n) => {
			let line = n.as_i64().ok_or("Line number must be an integer")?;
			match operation_type {

				OperationType::Insert => Ok(UnresolvedLineRange::Single(line)),
				OperationType::Replace => {
					if line == 0 {
						return Err(
							"Replace line numbers are 1-indexed, use 1 for first line".to_string()
						);
					}
					Ok(UnresolvedLineRange::Range(line, line))
				}
			}
		}
		Value::String(s) => {

			Ok(UnresolvedLineRange::Hash(s.clone()))
		}
		Value::Array(arr) => {
			if arr.is_empty() {
				return Err("Line range array must have 1 or 2 elements".to_string());
			}

			if arr[0].is_string() {

				if arr.len() != 2 {
					return Err("Hash range array must have exactly 2 elements".to_string());
				}
				let start_hash = arr[0].as_str().ok_or("Hash must be a string")?.to_string();
				let end_hash = arr[1].as_str().ok_or("Hash must be a string")?.to_string();
				match operation_type {
					OperationType::Insert => {
						Err("Insert operation cannot use hash range - use single hash".to_string())
					}
					OperationType::Replace => {
						Ok(UnresolvedLineRange::HashRange(start_hash, end_hash))
					}
				}
			} else if arr.len() == 1 {
				let line = arr[0].as_i64().ok_or("Line number must be an integer")?;
				match operation_type {

					OperationType::Insert => Ok(UnresolvedLineRange::Single(line)),
					OperationType::Replace => {
						if line == 0 {
							return Err("Replace line numbers are 1-indexed, use 1 for first line"
								.to_string());
						}
						Ok(UnresolvedLineRange::Range(line, line))
					}
				}
			} else if arr.len() == 2 {
				let start = arr[0].as_i64().ok_or("Start line must be an integer")?;
				let end = arr[1].as_i64().ok_or("End line must be an integer")?;
				if start == 0 || end == 0 {
					return Err(
						"Replace line numbers are 1-indexed, use 1 for first line".to_string()
					);
				}
				match operation_type {
					OperationType::Insert => Err(
						"Insert operation cannot use line range - use single line number"
							.to_string(),
					),
					OperationType::Replace => Ok(UnresolvedLineRange::Range(start, end)),
				}
			} else {
				Err("Line range array must have 1 or 2 elements".to_string())
			}
		}
		_ => Err("Line range must be a number, array, or hash string".to_string()),
	}
}


pub async fn batch_edit_spec(call: &McpToolCall, operations: &[Value]) -> Result<String> {

	let path_str = match call.parameters.get("path").and_then(|v| v.as_str()) {
		Some(p) => p,
		None => {
			bail!("Missing required 'path' parameter for batch_edit");
		}
	};


	if operations.is_empty() {
		bail!("Operations array is empty — nothing to do.");
	}

	const MAX_OPERATIONS: usize = 50;
	if operations.len() > MAX_OPERATIONS {
		bail!(
			"Too many operations: {} (max {}). Split into multiple calls.",
			operations.len(),
			MAX_OPERATIONS
		);
	}

	let path = super::core::resolve_path(path_str, &call.workdir);

	if !path.exists() {
		bail!("File not found: {}", path_str);
	}


	let file_lock = acquire_file_lock(&path).await?;
	let _lock_guard = file_lock.lock().await;


	let original_content = tokio_fs::read_to_string(&path)
		.await
		.map_err(|e| anyhow!("Failed to read file '{}': {}", path_str, e))?;


	let mut unresolved_operations = Vec::new();
	let mut failed_operations = 0;
	let mut operation_details = Vec::new();

	for (index, operation) in operations.iter().enumerate() {
		let operation_obj = match operation.as_object() {
			Some(obj) => obj,
			None => {
				failed_operations += 1;
				operation_details.push(json!({
					"operation_index": index,
					"status": "failed",
					"error": "Operation must be an object"
				}));
				continue;
			}
		};


		let op_type_str = match operation_obj.get("operation").and_then(|v| v.as_str()) {
			Some(op) => op,
			None => {
				failed_operations += 1;
				operation_details.push(json!({
					"operation_index": index,
					"status": "failed",
					"error": "Missing 'operation' field"
				}));
				continue;
			}
		};


		let operation_type = match op_type_str {
			"insert" => OperationType::Insert,
			"replace" => OperationType::Replace,
			_ => {
				failed_operations += 1;
				operation_details.push(json!({
					"operation_index": index,
					"operation": op_type_str,
					"status": "failed",
					"error": format!("Unsupported operation type: '{}'. Supported operations: insert, replace", op_type_str)
				}));
				continue;
			}
		};


		let line_range = match operation_obj.get("line_range") {
			Some(range_value) => match parse_line_range(range_value, &operation_type) {
				Ok(range) => range,
				Err(e) => {
					failed_operations += 1;
					operation_details.push(json!({
						"operation_index": index,
						"operation": op_type_str,
						"status": "failed",
						"error": format!("Invalid 'line_range': {}", e)
					}));
					continue;
				}
			},
			None => {
				failed_operations += 1;
				operation_details.push(json!({
					"operation_index": index,
					"operation": op_type_str,
					"status": "failed",
					"error": "Missing 'line_range' field"
				}));
				continue;
			}
		};


		let content = match operation_obj.get("content").and_then(|v| v.as_str()) {
			Some(c) => c.to_string(),
			None => {
				failed_operations += 1;
				operation_details.push(json!({
					"operation_index": index,
					"operation": op_type_str,
					"status": "failed",
					"error": "Missing 'content' field"
				}));
				continue;
			}
		};


		let unresolved_op = UnresolvedBatchOperation {
			operation_type,
			line_range: line_range.clone(),
			content,
			operation_index: index,
		};

		unresolved_operations.push(unresolved_op);

		operation_details.push(json!({
			"operation_index": index,
			"operation": op_type_str,
			"status": "parsed",
			"line_range": match &line_range {
				UnresolvedLineRange::Single(line) => json!(line),
				UnresolvedLineRange::Range(start, end) => json!([start, end]),
				UnresolvedLineRange::Hash(h) => json!(h),
				UnresolvedLineRange::HashRange(s, e) => json!([s, e]),
			}
		}));
	}


	if unresolved_operations.is_empty() {
		bail!(
			"No valid operations found. {} operations failed during parsing.",
			failed_operations
		);
	}


	let total_lines = original_content.lines().count();
	let original_lines_for_resolve: Vec<&str> = original_content.lines().collect();
	let mut batch_operations = Vec::new();

	for unresolved_op in unresolved_operations {
		match resolve_unresolved_line_range(
			&unresolved_op.line_range,
			total_lines,
			&original_lines_for_resolve,
		) {
			Ok(resolved_range) => {
				batch_operations.push(BatchOperation {
					operation_type: unresolved_op.operation_type,
					line_range: resolved_range,
					content: unresolved_op.content,
					operation_index: unresolved_op.operation_index,
				});
			}
			Err(err) => {
				bail!(
					"Invalid line range in operation {}: {}",
					unresolved_op.operation_index,
					err
				);
			}
		}
	}


	let original_lines: Vec<&str> = original_content.lines().collect();
	let use_hashes = crate::utils::line_hash::is_hash_mode();
	let orig_hashes: Vec<String> = if use_hashes {
		crate::utils::line_hash::compute_line_hashes(&original_lines)
	} else {
		Vec::new()
	};


	if let Err(conflict_error) = detect_conflicts(&batch_operations, &orig_hashes, use_hashes) {
		bail!("{}", conflict_error);
	}




	let orig_line_id = |line_1idx: usize| -> String {
		if use_hashes {
			orig_hashes[line_1idx - 1].clone()
		} else {
			format!("line {}", line_1idx)
		}
	};
	for op in &batch_operations {
		let content_lines: Vec<&str> = op.content.lines().collect();
		if content_lines.is_empty() {
			continue;
		}
		match op.operation_type {
			OperationType::Replace => {
				let (start, end) = match op.line_range {
					LineRange::Range(s, e) => (s, e),
					LineRange::Single(line) => (line, line),
				};
				if let Err(e) = check_replace_duplicates(
					&content_lines,
					&original_lines,
					start,
					end,
					op.operation_index,
					&orig_hashes,
					use_hashes,
				) {
					bail!("{}", e);
				}
			}
			OperationType::Insert => {

				let insert_after = match op.line_range {
					LineRange::Single(line) => line,
					_ => continue,
				};


				if content_lines.len() == 1 {
					if insert_after < original_lines.len() {
						let line_after = original_lines[insert_after];
						if content_lines[0] == line_after && !is_structural_noise(line_after) {
							bail!(
								"Duplicate line detected in operation {}: inserting after {} would duplicate {} which already reads {:?}. Do NOT re-insert content that already exists in the file.",
								op.operation_index, orig_line_id(insert_after), orig_line_id(insert_after + 1), line_after
							);
						}
					}
				} else {

					let available = original_lines.len().saturating_sub(insert_after);
					let check_len = content_lines.len().min(available);
					if check_len >= 2
						&& content_lines[..check_len]
							== original_lines[insert_after..insert_after + check_len]
					{
						bail!(
							"Duplicate block detected in operation {}: the {} inserted lines starting after {} already exist verbatim at {}-{}. Do NOT re-insert content that already exists in the file.",
							op.operation_index, check_len, orig_line_id(insert_after), orig_line_id(insert_after + 1), orig_line_id(insert_after + check_len)
						);
					}
				}
			}
		}
	}


	let final_content = apply_batch_operations(&original_content, &batch_operations)
		.await
		.map_err(|e| anyhow!("Failed to apply operations: {}", e))?;


	save_file_history(&path).await?;

	atomic_write(&path, &final_content)
		.await
		.map_err(|e| anyhow!("Atomic write failed for '{}': {}", path_str, e))?;


	for detail in &mut operation_details {
		if detail["status"] == "parsed" {
			detail["status"] = json!("success");
		}
	}




	const CONTEXT: usize = 2;
	let new_lines: Vec<&str> = final_content.lines().collect();
	let new_hashes: Vec<String> = if use_hashes {
		crate::utils::line_hash::compute_line_hashes(&new_lines)
	} else {
		Vec::new()
	};


	let orig_prefix = |line_1idx: usize| -> String {
		if use_hashes {
			orig_hashes[line_1idx - 1].clone()
		} else {
			format!("{}", line_1idx)
		}
	};
	let new_prefix = |line_1idx: usize| -> String {
		if use_hashes {
			new_hashes[line_1idx - 1].clone()
		} else {
			format!("{}", line_1idx)
		}
	};

	let mut diffs: Vec<String> = Vec::new();


	let mut display_ops = batch_operations.clone();
	display_ops.sort_by_key(|op| match &op.line_range {
		LineRange::Single(line) => *line,
		LineRange::Range(start, _) => *start,
	});

	for op in &display_ops {
		match op.operation_type {
			OperationType::Replace => {
				let (start, end) = match op.line_range {
					LineRange::Range(s, e) => (s, e),
					LineRange::Single(line) => (line, line),
				};
				let content_lines: Vec<&str> = op.content.lines().collect();
				let removed: Vec<String> = original_lines[start - 1..end]
					.iter()
					.map(|l| l.to_string())
					.collect();

				let mut diff: Vec<String> = Vec::new();
				let ctx_before_start = start.saturating_sub(CONTEXT).max(1);
				if ctx_before_start > 1 {
					diff.push("...".to_string());
				}
				for i in ctx_before_start..start {
					diff.push(format!("{}: {}", orig_prefix(i), original_lines[i - 1]));
				}
				for (i, old_line) in removed.iter().enumerate() {
					diff.push(format!("-{}: {}", orig_prefix(start + i), old_line));
				}
				for (i, new_line) in content_lines.iter().enumerate() {
					let idx = start + i;
					let pfx = if idx <= new_lines.len() {
						new_prefix(idx)
					} else {
						format!("{}", idx)
					};
					diff.push(format!("+{}: {}", pfx, new_line));
				}
				let new_after_start = start + content_lines.len();
				let new_after_end = (new_after_start + CONTEXT - 1).min(new_lines.len());
				for new_i in new_after_start..=new_after_end {
					if new_i >= 1 && new_i <= new_lines.len() {
						diff.push(format!("{}: {}", new_prefix(new_i), new_lines[new_i - 1]));
					}
				}
				if new_after_end < new_lines.len() {
					diff.push("...".to_string());
				}
				diffs.push(diff.join("\n"));
			}
			OperationType::Insert => {

				let after = match op.line_range {
					LineRange::Single(line) => line,
					LineRange::Range(start, _) => start,
				};
				let content_lines: Vec<&str> = op.content.lines().collect();
				let insert_at = after + 1;
				let mut diff: Vec<String> = Vec::new();


				let ctx_before_start = insert_at.saturating_sub(CONTEXT).max(1);
				if ctx_before_start > 1 {
					diff.push("...".to_string());
				}
				for new_i in ctx_before_start..insert_at {
					if new_i >= 1 && new_i <= new_lines.len() {
						diff.push(format!("{}: {}", new_prefix(new_i), new_lines[new_i - 1]));
					}
				}


				for (i, new_line) in content_lines.iter().enumerate() {
					let idx = insert_at + i;
					let pfx = if idx <= new_lines.len() {
						new_prefix(idx)
					} else {
						format!("{}", idx)
					};
					diff.push(format!("+{}: {}", pfx, new_line));
				}


				let after_end = insert_at + content_lines.len();
				let ctx_after_end = (after_end + CONTEXT - 1).min(new_lines.len());
				for new_i in after_end..=ctx_after_end {
					if new_i >= 1 && new_i <= new_lines.len() {
						diff.push(format!("{}: {}", new_prefix(new_i), new_lines[new_i - 1]));
					}
				}
				if ctx_after_end < new_lines.len() {
					diff.push("...".to_string());
				}

				diffs.push(diff.join("\n"));
			}
		}
	}



	let diff_output = diffs.join("\n---\n");

	Ok(diff_output)
}
