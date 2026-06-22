
















pub fn apply_head_truncation(lines: &[String], max_lines: usize) -> (Vec<String>, Option<String>) {
	if max_lines > 0 && lines.len() > max_lines {
		let mut truncated = Vec::new();


		truncated.extend(lines.iter().take(max_lines - 1).cloned());

		let truncated_count = lines.len() - (max_lines - 1);
		truncated.push(format!(
			"[{} lines truncated - use more specific patterns or increase max_lines]",
			truncated_count
		));

		(
			truncated,
			Some(format!(
				"Output truncated: showing {} of {} total lines",
				max_lines,
				lines.len()
			)),
		)
	} else {
		(lines.to_vec(), None)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_no_truncation_when_under_limit() {
		let lines = vec!["line1".to_string(), "line2".to_string()];
		let (result, info) = apply_head_truncation(&lines, 5);

		assert_eq!(result, lines);
		assert!(info.is_none());
	}

	#[test]
	fn test_truncation_when_over_limit() {
		let lines = vec![
			"line1".to_string(),
			"line2".to_string(),
			"line3".to_string(),
			"line4".to_string(),
			"line5".to_string(),
		];
		let (result, info) = apply_head_truncation(&lines, 3);

		assert_eq!(result.len(), 3);
		assert_eq!(result[0], "line1");
		assert_eq!(result[1], "line2");
		assert!(result[2].contains("3 lines truncated"));
		assert!(info.is_some());
		assert!(info.unwrap().contains("showing 3 of 5 total lines"));
	}

	#[test]
	fn test_unlimited_when_max_lines_zero() {
		let lines = vec!["line1".to_string(), "line2".to_string()];
		let (result, info) = apply_head_truncation(&lines, 0);

		assert_eq!(result, lines);
		assert!(info.is_none());
	}
}
