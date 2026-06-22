













#[cfg(test)]
mod tests {
	use crate::mcp::fs::core::{execute_batch_edit, execute_extract_lines, execute_view};

	use crate::mcp::McpToolCall;
	use serde_json::json;
	use tempfile::NamedTempFile;
	use tokio::fs;

	async fn create_test_file(content: &str) -> NamedTempFile {
		let temp_file = NamedTempFile::new().unwrap();
		fs::write(temp_file.path(), content).await.unwrap();
		temp_file
	}


	async fn test_batch_replace(
		content: &str,
		start_line: usize,
		end_line: usize,
		new_str: &str,
		expected: &str,
	) {
		let temp_file = create_test_file(content).await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "replace",
					"line_range": [start_line, end_line],
					"content": new_str
				}]
			}),
		};
		execute_batch_edit(&call).await.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, expected, "Content mismatch");
	}

	#[tokio::test]
	async fn test_replace_single_line() {
		test_batch_replace(
			"line 1\nline 2\nline 3\n",
			2,
			2,
			"REPLACED",
			"line 1\nREPLACED\nline 3\n",
		)
		.await;
	}

	#[tokio::test]
	async fn test_replace_multiple_lines() {
		test_batch_replace(
			"line 1\nline 2\nline 3\nline 4\n",
			2,
			3,
			"SINGLE REPLACEMENT",
			"line 1\nSINGLE REPLACEMENT\nline 4\n",
		)
		.await;
	}

	#[tokio::test]
	async fn test_replace_with_multiline_content() {
		test_batch_replace(
			"line 1\nline 2\nline 3\n",
			2,
			2,
			"FIRST\nSECOND",
			"line 1\nFIRST\nSECOND\nline 3\n",
		)
		.await;
	}

	#[tokio::test]
	async fn test_replace_first_line() {
		test_batch_replace(
			"line 1\nline 2\nline 3\n",
			1,
			1,
			"NEW FIRST",
			"NEW FIRST\nline 2\nline 3\n",
		)
		.await;
	}

	#[tokio::test]
	async fn test_replace_last_line() {
		test_batch_replace(
			"line 1\nline 2\nline 3\n",
			3,
			3,
			"NEW LAST",
			"line 1\nline 2\nNEW LAST\n",
		)
		.await;
	}

	#[tokio::test]
	async fn test_replace_all_lines() {
		test_batch_replace(
			"line 1\nline 2\nline 3\n",
			1,
			3,
			"EVERYTHING REPLACED",
			"EVERYTHING REPLACED\n",
		)
		.await;
	}

	#[tokio::test]
	async fn test_replace_no_final_newline() {
		test_batch_replace(
			"line 1\nline 2\nline 3",
			2,
			2,
			"REPLACED",
			"line 1\nREPLACED\nline 3",
		)
		.await;
	}

	#[tokio::test]
	async fn test_replace_crlf_line_endings() {

		test_batch_replace(
			"line 1\r\nline 2\r\nline 3\r\n",
			2,
			2,
			"REPLACED",
			"line 1\nREPLACED\nline 3\n",
		)
		.await;
	}

	#[tokio::test]
	async fn test_replace_empty_content_deletes_lines() {

		test_batch_replace("line 1\nline 2\nline 3\n", 2, 2, "", "line 1\nline 3\n").await;
	}

	#[tokio::test]
	async fn test_replace_single_line_file() {
		test_batch_replace("only line", 1, 1, "REPLACED", "REPLACED").await;
	}

	#[tokio::test]
	async fn test_replace_unicode() {
		test_batch_replace(
			"line 1\nline 2\nline 3\n",
			2,
			2,
			"🚀 Hello 世界 🎉",
			"line 1\n🚀 Hello 世界 🎉\nline 3\n",
		)
		.await;
	}
	#[tokio::test]
	async fn test_replace_special_chars() {
		test_batch_replace(
			"line 1\nline 2\nline 3\n",
			2,
			2,
			"!@#$%^&*()[]{}|;':\",./<>?",
			"line 1\n!@#$%^&*()[]{}|;':\",./<>?\nline 3\n",
		)
		.await;
	}

	#[tokio::test]
	async fn test_replace_content_with_quotes() {
		test_batch_replace(
			"line 1\nline 2\nline 3\n",
			2,
			2,
			"\"quoted value\"",
			"line 1\n\"quoted value\"\nline 3\n",
		)
		.await;
	}

	#[tokio::test]
	async fn test_replace_content_with_tabs() {
		test_batch_replace(
			"line 1\nline 2\nline 3\n",
			2,
			2,
			"\tindented line",
			"line 1\n\tindented line\nline 3\n",
		)
		.await;
	}

	#[tokio::test]
	async fn test_replace_content_with_embedded_newlines() {
		test_batch_replace(
			"line 1\nline 2\nline 3\n",
			2,
			2,
			"hello\nworld\ntest",
			"line 1\nhello\nworld\ntest\nline 3\n",
		)
		.await;
	}

	#[tokio::test]
	async fn test_replace_no_false_positive_on_structural_noise() {


		let content = "fn foo() {\n\tlet x = 1;\n}\nfn bar() {\n\tlet y = 2;\n}\n";
		let temp_file = create_test_file(content).await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "replace",
					"line_range": [4, 6],


					"content": "}\nfn bar() {\n\tlet y = 99;\n}"
				}]
			}),
		};
		execute_batch_edit(&call).await.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(
			actual,
			"fn foo() {\n\tlet x = 1;\n}\n}\nfn bar() {\n\tlet y = 99;\n}\n"
		);
	}

	#[tokio::test]
	async fn test_replace_compound_closer_is_not_noise() {


		let content = "foo(\n\tbar(),\n});\nbaz();\n";
		let temp_file = create_test_file(content).await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "replace",
					"line_range": [4, 4],


					"content": "});\nnew_baz();"
				}]
			}),
		};
		let err = execute_batch_edit(&call).await.unwrap_err();
		assert!(
			err.to_string().contains("Duplicate line detected"),
			"compound closer at boundary must trigger duplicate detection: {}",
			err
		);

		let actual = tokio::fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, content);
	}
	#[tokio::test]
	async fn test_replace_duplicate_detection_before() {

		let temp_file = create_test_file("line 1\nline 2\nline 3\nline 4\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "replace",
					"line_range": [3, 4],
					"content": "line 2\nnew line 3\nnew line 4"
				}]
			}),
		};
		let err = execute_batch_edit(&call).await.unwrap_err();
		assert!(err.to_string().contains("Duplicate line detected"));

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "line 1\nline 2\nline 3\nline 4\n");
	}

	#[tokio::test]
	async fn test_replace_duplicate_detection_after() {

		let temp_file = create_test_file("line 1\nline 2\nline 3\nline 4\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "replace",
					"line_range": [1, 2],
					"content": "new line 1\nnew line 2\nline 3"
				}]
			}),
		};
		let err = execute_batch_edit(&call).await.unwrap_err();
		assert!(err.to_string().contains("Duplicate line detected"));
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "line 1\nline 2\nline 3\nline 4\n");
	}

	#[tokio::test]
	async fn test_replace_no_false_duplicate_warning() {

		let temp_file = create_test_file("line 1\nline 2\nline 3\nline 4\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "replace",
					"line_range": [2, 3],
					"content": "new line 2\nnew line 3"
				}]
			}),
		};
		let text = execute_batch_edit(&call).await.unwrap();

		assert!(!text.is_empty());
	}



	#[tokio::test]
	async fn test_insert_single_line_duplicate_blocked() {




		let content = "line 1\nline 2\nline 3\n";
		let temp_file = create_test_file(content).await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "insert",
					"line_range": 1,
					"content": "line 2"
				}]
			}),
		};
		let err = execute_batch_edit(&call).await.unwrap_err();
		assert!(
			err.to_string().contains("Duplicate line detected"),
			"single-line insert duplicate must be blocked: {}",
			err
		);

		let actual = tokio::fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, content);
	}

	#[tokio::test]
	async fn test_insert_single_line_noise_allowed() {


		let content = "fn foo() {\n\tlet x = 1;\n}\n";
		let temp_file = create_test_file(content).await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "insert",
					"line_range": 2,
					"content": "}"
				}]
			}),
		};
		execute_batch_edit(&call).await.unwrap();
	}

	#[tokio::test]
	async fn test_insert_multi_line_duplicate_blocked() {




		let content = "line 1\nline 2\nline 3\nline 4\n";
		let temp_file = create_test_file(content).await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "insert",
					"line_range": 1,
					"content": "line 2\nline 3"
				}]
			}),
		};
		let err = execute_batch_edit(&call).await.unwrap_err();
		assert!(
			err.to_string().contains("Duplicate block detected"),
			"multi-line insert duplicate must be blocked: {}",
			err
		);

		let actual = tokio::fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, content);
	}

	#[tokio::test]
	async fn test_insert_multi_line_noise_block_blocked() {




		let content = "fn foo() {\n}\n}\nend\n";
		let temp_file = create_test_file(content).await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "insert",
					"line_range": 1,
					"content": "}\n}"
				}]
			}),
		};
		let err = execute_batch_edit(&call).await.unwrap_err();
		assert!(
			err.to_string().contains("Duplicate block detected"),
			"multi-line noise block insert duplicate must be blocked: {}",
			err
		);
		let actual = tokio::fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, content);
	}

	#[tokio::test]
	async fn test_insert_multi_line_new_content_allowed() {

		let content = "line 1\nline 3\n";
		let temp_file = create_test_file(content).await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "insert",
					"line_range": 1,
					"content": "line 2a\nline 2b"
				}]
			}),
		};
		execute_batch_edit(&call).await.unwrap();
		let actual = tokio::fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "line 1\nline 2a\nline 2b\nline 3\n");
	}



	#[tokio::test]
	async fn test_replace_diff_output_present() {

		let temp_file = create_test_file("line 1\nline 2\nline 3\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "replace",
					"line_range": [2, 2],
					"content": "REPLACED"
				}]
			}),
		};
		let diff = execute_batch_edit(&call).await.unwrap();
		assert!(diff.contains("-2:"), "diff must show removed line");
		assert!(diff.contains("+2:"), "diff must show added line");
	}

	#[tokio::test]
	async fn test_replace_negative_line_index() {

		let temp_file = create_test_file("line 1\nline 2\nline 3\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "replace",
					"line_range": [-1, -1],
					"content": "NEW LAST"
				}]
			}),
		};
		execute_batch_edit(&call).await.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "line 1\nline 2\nNEW LAST\n");
	}



	async fn test_str_replace(content: &str, old_str: &str, new_str: &str, expected: &str) {
		let temp_file = create_test_file(content).await;

		crate::mcp::fs::text_editing::str_replace_spec(temp_file.path(), old_str, new_str)
			.await
			.unwrap();


		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, expected, "Content mismatch");
	}

	#[tokio::test]
	async fn test_str_replace_basic() {
		test_str_replace(
			"Hello world\nThis is a test\nGoodbye universe",
			"world",
			"universe",
			"Hello universe\nThis is a test\nGoodbye universe",
		)
		.await;
	}

	#[tokio::test]
	async fn test_str_replace_multiline_old() {
		test_str_replace(
			"line 1\nline 2\nline 3\nline 4",
			"line 2\nline 3",
			"REPLACED",
			"line 1\nREPLACED\nline 4",
		)
		.await;
	}

	#[tokio::test]
	async fn test_str_replace_multiline_new() {
		test_str_replace(
			"line 1\nREPLACE_ME\nline 3",
			"REPLACE_ME",
			"new line 1\nnew line 2",
			"line 1\nnew line 1\nnew line 2\nline 3",
		)
		.await;
	}

	#[tokio::test]
	async fn test_str_replace_with_quotes() {
		test_str_replace(
			"let x = \"old_value\";",
			"\"old_value\"",
			"\"new_value\"",
			"let x = \"new_value\";",
		)
		.await;
	}

	#[tokio::test]
	async fn test_str_replace_with_actual_newlines() {

		test_str_replace(
			"hello\nworld\ntest",
			"hello\nworld",
			"goodbye\nuniverse",
			"goodbye\nuniverse\ntest",
		)
		.await;
	}

	#[tokio::test]
	async fn test_str_replace_with_literal_backslash_n() {

		test_str_replace(
			"hello\\nworld\\ntest",
			"hello\\nworld",
			"goodbye\\nuniverse",
			"goodbye\\nuniverse\\ntest",
		)
		.await;
	}

	#[tokio::test]
	async fn test_str_replace_with_tabs() {
		test_str_replace(
			"function() {\n\told_code();\n}",
			"\told_code();",
			"\tnew_code();\n\tmore_code();",
			"function() {\n\tnew_code();\n\tmore_code();\n}",
		)
		.await;
	}

	#[tokio::test]
	async fn test_str_replace_with_special_chars() {
		test_str_replace(
			"regex = /[a-z]+/g;",
			"/[a-z]+/g",
			"/[A-Z]+/i",
			"regex = /[A-Z]+/i;",
		)
		.await;
	}

	#[tokio::test]
	async fn test_str_replace_with_unicode() {
		test_str_replace("Hello 世界! 🚀", "世界", "Universe", "Hello Universe! 🚀").await;
	}

	#[tokio::test]
	async fn test_str_replace_windows_line_endings() {
		test_str_replace(
			"line 1\r\nline 2\r\nline 3\r\n",
			"line 2",
			"REPLACED",
			"line 1\r\nREPLACED\r\nline 3\r\n",
		)
		.await;
	}

	#[tokio::test]
	async fn test_str_replace_complex_code() {
		let old_code = "fn old_function() {\n    println!(\"old\");\n}";
		let new_code = "fn new_function() {\n    println!(\"new\");\n    return 42;\n}";

		test_str_replace(
			"// Some comment\nfn old_function() {\n    println!(\"old\");\n}\n// End",
			old_code,
			new_code,
			"// Some comment\nfn new_function() {\n    println!(\"new\");\n    return 42;\n}\n// End",
		)
		.await;
	}

	#[tokio::test]
	async fn test_str_replace_with_control_chars() {
		test_str_replace(
			"data\x00null\x01control",
			"\x00null\x01",
			"\x02new\x03",
			"data\x02new\x03control",
		)
		.await;
	}

	#[tokio::test]
	async fn test_str_replace_error_no_match() {
		let temp_file = create_test_file("Hello world").await;

		let err = crate::mcp::fs::text_editing::str_replace_spec(
			temp_file.path(),
			"not_found",
			"replacement",
		)
		.await
		.unwrap_err();


		assert!(
			err.to_string().contains("No exact match found"),
			"Should contain no match error message, got: {}",
			err
		);
	}

	#[tokio::test]
	async fn test_str_replace_error_multiple_matches() {
		let temp_file = create_test_file("test test test").await;

		let err =
			crate::mcp::fs::text_editing::str_replace_spec(temp_file.path(), "test", "replacement")
				.await
				.unwrap_err();


		assert!(
			err.to_string().contains("Found 3 matches"),
			"Should contain multiple matches error message"
		);
	}

	#[tokio::test]
	async fn test_str_replace_byte_level_verification() {
		let temp_file = create_test_file("hello\nworld\ntest").await;


		crate::mcp::fs::text_editing::str_replace_spec(temp_file.path(), "world", "new\nline")
			.await
			.unwrap();


		let actual_bytes = fs::read(temp_file.path()).await.unwrap();
		let expected_bytes = b"hello\nnew\nline\ntest";

		assert_eq!(actual_bytes, expected_bytes, "Byte-level content mismatch");


		assert_eq!(actual_bytes[5], 10u8, "First newline should be byte 10");
		assert_eq!(actual_bytes[9], 10u8, "Second newline should be byte 10");
		assert_eq!(actual_bytes[14], 10u8, "Third newline should be byte 10");
	}

	#[tokio::test]
	async fn test_list_files_basic_functionality() {
		use crate::mcp::fs::directory::list_directory;
		use std::fs;
		use tempfile::TempDir;


		let temp_dir = TempDir::new().unwrap();
		let temp_path = temp_dir.path();


		for i in 1..=30 {
			let file_path = temp_path.join(format!("test_file_{:02}.txt", i));
			fs::write(&file_path, format!("Content of file {}", i)).unwrap();
		}


		let call = McpToolCall {
			tool_name: "view".to_string(),
			parameters: json!({
				"directory": temp_path.to_str().unwrap(),
				"pattern": "*.txt"
			}),
			tool_id: "test-call-id".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
		};

		let output = list_directory(
			&call,
			call.parameters
				.get("directory")
				.and_then(|v| v.as_str())
				.unwrap_or("."),
		)
		.await
		.unwrap();


		let file_lines: Vec<&str> = output.lines().filter(|l| !l.is_empty()).collect();
		assert_eq!(
			file_lines.len(),
			30,
			"Should list 30 files, got: {}",
			output
		);


		let call_limited = McpToolCall {
			tool_name: "view".to_string(),
			parameters: json!({
				"directory": temp_path.to_str().unwrap(),
				"pattern": "*_01.txt"
			}),
			tool_id: "test-call-id".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
		};

		let output_limited = list_directory(
			&call_limited,
			call_limited
				.parameters
				.get("directory")
				.and_then(|v| v.as_str())
				.unwrap_or("."),
		)
		.await
		.unwrap();


		let limited_lines: Vec<&str> = output_limited.lines().filter(|l| !l.is_empty()).collect();
		assert_eq!(
			limited_lines.len(),
			1,
			"Should find 1 file, got: {}",
			output_limited
		);
		assert!(limited_lines[0].contains("test_file_01.txt"));
	}

	#[tokio::test]
	async fn test_list_files_content_search_preserves_format() {
		use crate::mcp::fs::directory::list_directory;
		use std::fs;
		use tempfile::TempDir;


		let temp_dir = TempDir::new().unwrap();
		let temp_path = temp_dir.path();


		let file1_path = temp_path.join("test1.rs");
		fs::write(
			&file1_path,
			"fn main() {\n    println!(\"Hello, world!\");\n    let x = 42;\n}\n",
		)
		.unwrap();

		let file2_path = temp_path.join("test2.rs");
		fs::write(&file2_path, "pub fn helper() {\n    println!(\"Helper function\");\n}\n\nfn main() {\n    helper();\n}\n").unwrap();


		let call = McpToolCall {
			tool_name: "view".to_string(),
			parameters: json!({
				"directory": temp_path.to_str().unwrap(),
				"content": "println!",
				"max_lines": 0
			}),
			tool_id: "test-call-id".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
		};

		let output = list_directory(
			&call,
			call.parameters
				.get("directory")
				.and_then(|v| v.as_str())
				.unwrap_or("."),
		)
		.await
		.unwrap();

		println!("Content search output:\n{}", output);


		assert!(
			output.contains("test1.rs:") || output.contains("test2.rs:"),
			"Should contain filenames: {}",
			output
		);


		let call_with_context = McpToolCall {
			tool_name: "view".to_string(),
			parameters: json!({
				"directory": temp_path.to_str().unwrap(),
				"content": "println!",
				"context": 1,
				"max_lines": 0
			}),
			tool_id: "test-call-id".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
		};

		let output_with_context = list_directory(
			&call_with_context,
			call_with_context
				.parameters
				.get("directory")
				.and_then(|v| v.as_str())
				.unwrap_or("."),
		)
		.await
		.unwrap();

		println!("Content search with context:\n{}", output_with_context);


		let lines_no_context = output.lines().count();
		let lines_with_context = output_with_context.lines().count();
		assert!(
			lines_with_context >= lines_no_context,
			"Context should add more lines: {} vs {}",
			lines_with_context,
			lines_no_context
		);
	}

	#[tokio::test]
	async fn test_list_files_vs_content_search_different_output() {
		use crate::mcp::fs::directory::list_directory;
		use std::fs;
		use tempfile::TempDir;


		let temp_dir = TempDir::new().unwrap();
		let temp_path = temp_dir.path();


		for i in 1..=5 {
			let file_path = temp_path.join(format!("test_{}.rs", i));
			fs::write(
				&file_path,
				format!("fn test_{}() {{\n    println!(\"Test {}\");\n}}\n", i, i),
			)
			.unwrap();
		}


		let file_list_call = McpToolCall {
			tool_name: "view".to_string(),
			parameters: json!({
				"directory": temp_path.to_str().unwrap(),
				"pattern": "*.rs"
			}),
			tool_id: "test-call-id".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
		};

		let file_list_str = list_directory(
			&file_list_call,
			file_list_call
				.parameters
				.get("directory")
				.and_then(|v| v.as_str())
				.unwrap_or("."),
		)
		.await
		.unwrap();


		let content_search_call = McpToolCall {
			tool_name: "view".to_string(),
			parameters: json!({
				"directory": temp_path.to_str().unwrap(),
				"content": "println!"
			}),
			tool_id: "test-call-id".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
		};

		let content_search_str = list_directory(
			&content_search_call,
			content_search_call
				.parameters
				.get("directory")
				.and_then(|v| v.as_str())
				.unwrap_or("."),
		)
		.await
		.unwrap();

		println!("File listing output:\n{}", file_list_str);
		println!("Content search output:\n{}", content_search_str);


		assert!(file_list_str.contains("test_1.rs"));

		let line_number_pattern = regex::Regex::new(r"(:\d+:|^\d+:)").unwrap();
		assert!(!line_number_pattern.is_match(&file_list_str));


		let has_line_numbers = content_search_str.contains("2:    println!")
			|| line_number_pattern.is_match(&content_search_str);
		assert!(has_line_numbers);
		assert!(content_search_str.contains("println!"));
	}



	async fn test_extract_lines(
		source_content: &str,
		from_range: (usize, usize),
		target_content: &str,
		append_line: i64,
		expected_target: &str,
	) {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let source_path = temp_dir.path().join("source.txt");
		let target_path = temp_dir.path().join("target.txt");


		fs::write(&source_path, source_content).await.unwrap();


		if !target_content.is_empty() {
			fs::write(&target_path, target_content).await.unwrap();
		}

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "extract_lines".to_string(),
			parameters: json!({
				"from_path": source_path.to_string_lossy(),
				"from_range": [from_range.0, from_range.1],
				"append_path": target_path.to_string_lossy(),
				"append_line": append_line
			}),
		};

		execute_extract_lines(&call).await.unwrap();


		let source_after = fs::read_to_string(&source_path).await.unwrap();
		assert_eq!(
			source_after, source_content,
			"Source file should be unchanged"
		);


		let target_after = fs::read_to_string(&target_path).await.unwrap();
		assert_eq!(
			target_after, expected_target,
			"Target file content mismatch"
		);
	}

	#[tokio::test]
	async fn test_extract_single_line_to_empty_file() {
		test_extract_lines("line 1\nline 2\nline 3\n", (2, 2), "", -1, "line 2").await;
	}

	#[tokio::test]
	async fn test_extract_multiple_lines_to_empty_file() {
		test_extract_lines(
			"line 1\nline 2\nline 3\nline 4\n",
			(2, 3),
			"",
			-1,
			"line 2\nline 3",
		)
		.await;
	}

	#[tokio::test]
	async fn test_extract_append_to_end() {
		test_extract_lines(
			"source 1\nsource 2\nsource 3\n",
			(1, 2),
			"existing 1\nexisting 2\n",
			-1,
			"existing 1\nexisting 2\nsource 1\nsource 2",
		)
		.await;
	}

	#[tokio::test]
	async fn test_extract_insert_at_beginning() {
		test_extract_lines(
			"new 1\nnew 2\n",
			(1, 2),
			"old 1\nold 2\n",
			0,
			"new 1\nnew 2\nold 1\nold 2\n",
		)
		.await;
	}

	#[tokio::test]
	async fn test_extract_insert_after_line() {
		test_extract_lines(
			"inserted 1\ninserted 2\n",
			(1, 2),
			"line 1\nline 2\nline 3\n",
			2,
			"line 1\nline 2\ninserted 1\ninserted 2\nline 3\n",
		)
		.await;
	}

	#[tokio::test]
	async fn test_extract_first_line() {
		test_extract_lines("first\nsecond\nthird\n", (1, 1), "", -1, "first").await;
	}

	#[tokio::test]
	async fn test_extract_last_line() {
		test_extract_lines("first\nsecond\nlast\n", (3, 3), "", -1, "last\n").await;
	}

	#[tokio::test]
	async fn test_extract_all_lines() {
		test_extract_lines(
			"all 1\nall 2\nall 3\n",
			(1, 3),
			"",
			-1,
			"all 1\nall 2\nall 3\n",
		)
		.await;
	}

	#[tokio::test]
	async fn test_extract_lines_with_special_characters() {
		test_extract_lines(
			"fn main() {\n    println!(\"Hello, world!\");\n}\n",
			(1, 3),
			"",
			-1,
			"fn main() {\n    println!(\"Hello, world!\");\n}\n",
		)
		.await;
	}

	#[tokio::test]
	async fn test_extract_lines_error_invalid_range() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let source_path = temp_dir.path().join("source.txt");
		let target_path = temp_dir.path().join("target.txt");

		fs::write(&source_path, "line 1\nline 2\n").await.unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "extract_lines".to_string(),
			parameters: json!({
				"from_path": source_path.to_string_lossy(),
				"from_range": [1, 5],
				"append_path": target_path.to_string_lossy(),
				"append_line": -1
			}),
		};

		let err = execute_extract_lines(&call).await.unwrap_err();
		assert!(
			err.to_string().contains("exceeds file length"),
			"Should fail with invalid range: {}",
			err
		);
	}

	#[tokio::test]
	async fn test_extract_lines_error_start_greater_than_end() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let source_path = temp_dir.path().join("source.txt");
		let target_path = temp_dir.path().join("target.txt");

		fs::write(&source_path, "line 1\nline 2\n").await.unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "extract_lines".to_string(),
			parameters: json!({
				"from_path": source_path.to_string_lossy(),
				"from_range": [2, 1],
				"append_path": target_path.to_string_lossy(),
				"append_line": -1
			}),
		};

		let err = execute_extract_lines(&call).await.unwrap_err();
		assert!(
			err.to_string().contains("cannot be greater than"),
			"Should fail when start > end: {}",
			err
		);
	}

	#[tokio::test]
	async fn test_extract_lines_error_missing_source_file() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let source_path = temp_dir.path().join("nonexistent.txt");
		let target_path = temp_dir.path().join("target.txt");

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "extract_lines".to_string(),
			parameters: json!({
				"from_path": source_path.to_string_lossy(),
				"from_range": [1, 1],
				"append_path": target_path.to_string_lossy(),
				"append_line": -1
			}),
		};

		let err = execute_extract_lines(&call).await.unwrap_err();
		assert!(
			err.to_string().contains("does not exist"),
			"Should fail with missing source file: {}",
			err
		);
	}

	#[tokio::test]
	async fn test_extract_lines_error_invalid_append_position() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let source_path = temp_dir.path().join("source.txt");
		let target_path = temp_dir.path().join("target.txt");

		fs::write(&source_path, "line 1\nline 2\n").await.unwrap();
		fs::write(&target_path, "existing\n").await.unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "extract_lines".to_string(),
			parameters: json!({
				"from_path": source_path.to_string_lossy(),
				"from_range": [1, 1],
				"append_path": target_path.to_string_lossy(),
				"append_line": 5
			}),
		};

		let err = execute_extract_lines(&call).await.unwrap_err();
		assert!(
			err.to_string().contains("exceeds target file length"),
			"Should fail with invalid append position: {}",
			err
		);
	}

	#[tokio::test]
	async fn test_extract_lines_creates_parent_directories() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let source_path = temp_dir.path().join("source.txt");
		let target_path = temp_dir.path().join("nested/deep/target.txt");

		fs::write(&source_path, "content\n").await.unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "extract_lines".to_string(),
			parameters: json!({
				"from_path": source_path.to_string_lossy(),
				"from_range": [1, 1],
				"append_path": target_path.to_string_lossy(),
				"append_line": -1
			}),
		};

		execute_extract_lines(&call).await.unwrap();


		let target_content = fs::read_to_string(&target_path).await.unwrap();
		assert_eq!(target_content, "content\n");
	}

	#[tokio::test]
	async fn test_extract_lines_parameter_validation() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let source_path = temp_dir.path().join("source.txt");
		let target_path = temp_dir.path().join("target.txt");

		fs::write(&source_path, "line 1\n").await.unwrap();


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "extract_lines".to_string(),
			parameters: json!({
				"from_range": [1, 1],
				"append_path": target_path.to_string_lossy(),
				"append_line": -1
			}),
		};

		let err = execute_extract_lines(&call).await.unwrap_err();
		assert!(
			err.to_string()
				.contains("Missing required parameter 'from_path'"),
			"Should fail with missing from_path: {}",
			err
		);


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "extract_lines".to_string(),
			parameters: json!({
				"from_path": source_path.to_string_lossy(),
				"from_range": [1],
				"append_path": target_path.to_string_lossy(),
				"append_line": -1
			}),
		};

		let err = execute_extract_lines(&call).await.unwrap_err();
		assert!(
			err.to_string().contains("exactly 2 elements"),
			"Should fail with invalid from_range: {}",
			err
		);


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "extract_lines".to_string(),
			parameters: json!({
				"from_path": "",
				"from_range": [1, 1],
				"append_path": target_path.to_string_lossy(),
				"append_line": -1
			}),
		};

		let err = execute_extract_lines(&call).await.unwrap_err();
		assert!(
			err.to_string().contains("cannot be empty"),
			"Should fail with empty from_path: {}",
			err
		);
	}





	async fn create_batch_edit_call(path: &str, operations: serde_json::Value) -> McpToolCall {
		McpToolCall {
			tool_id: "test_batch_edit".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": operations
			}),
		}
	}

	#[tokio::test]
	async fn test_batch_edit_single_insert() {
		let temp_file = create_test_file("line 1\nline 2\nline 3\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let operations = json!([
			{
				"operation": "insert",
				"line_range": 2,
				"content": "inserted line"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();


		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		let expected = "line 1\nline 2\ninserted line\nline 3\n";
		assert_eq!(
			actual, expected,
			"Content should match expected after insert"
		);
	}

	#[tokio::test]
	async fn test_batch_edit_multiple_operations_original_line_numbers() {

		let temp_file = create_test_file("line 1\nline 2\nline 3\nline 4\nline 5\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let operations = json!([
			{
				"operation": "insert",
				"line_range": 1,
				"content": "inserted after line 1"
			},
			{
				"operation": "replace",
				"line_range": [3, 3],
				"content": "replaced original line 3"
			},
			{
				"operation": "insert",
				"line_range": 5,
				"content": "inserted after original line 5"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();


		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		let expected = "line 1\ninserted after line 1\nline 2\nreplaced original line 3\nline 4\nline 5\ninserted after original line 5\n";
		assert_eq!(
			actual, expected,
			"Content should reflect all operations using original line numbers"
		);
	}

	#[tokio::test]
	async fn test_batch_edit_insert_and_replace_same_line_no_conflict() {


		let temp_file = create_test_file("line 1\nline 2\nline 3\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let operations = json!([
			{
				"operation": "insert",
				"line_range": 2,
				"content": "inserted after line 2"
			},
			{
				"operation": "replace",
				"line_range": [2, 2],
				"content": "replaced line 2"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();



		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(
			actual,
			"line 1\nreplaced line 2\ninserted after line 2\nline 3\n"
		);
	}

	#[tokio::test]
	async fn test_batch_edit_overlapping_replace_ranges() {
		let temp_file = create_test_file("line 1\nline 2\nline 3\nline 4\nline 5\n").await;
		let path = temp_file.path().to_string_lossy().to_string();


		let operations = json!([
			{
				"operation": "replace",
				"line_range": [2, 3],
				"content": "replaced 2-3"
			},
			{
				"operation": "replace",
				"line_range": [3, 4],
				"content": "replaced 3-4"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		let err = crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap_err();

		assert!(
			err.to_string().contains("overlapping ranges"),
			"Should detect overlap: {}",
			err
		);
	}

	#[tokio::test]
	async fn test_batch_edit_missing_path() {
		let operations = json!([
			{
				"operation": "insert",
				"line_range": 1,
				"content": "test"
			}
		]);

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "text_editor".to_string(),
			parameters: json!({
				"command": "batch_edit",
				"operations": operations

			}),
		};

		let err = crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap_err();

		assert!(
			err.to_string()
				.contains("Missing required 'path' parameter"),
			"Should indicate missing path: {}",
			err
		);
	}

	#[tokio::test]
	async fn test_batch_edit_invalid_operation_type() {
		let temp_file = create_test_file("line 1\nline 2\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let operations = json!([
			{
				"operation": "invalid_op",
				"line_range": 1,
				"content": "test"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		let err = crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap_err();

		let msg = err.to_string();
		assert!(
			msg.contains("No valid operations found"),
			"Should indicate no valid operations: {}",
			msg
		);
		assert!(
			msg.contains("operations failed during parsing"),
			"Should indicate parsing failure: {}",
			msg
		);
	}

	#[tokio::test]
	async fn test_batch_edit_comprehensive_scenario() {

		let temp_file = create_test_file(
			"# Header\nfunction main() {\n    console.log('hello');\n    return 0;\n}\n// Footer\n",
		)
		.await;
		let path = temp_file.path().to_string_lossy().to_string();

		let operations = json!([
			{
				"operation": "insert",
				"line_range": 1,
				"content": "// Added by batch_edit"
			},
			{
				"operation": "replace",
				"line_range": [3, 3],
				"content": "    console.log('Hello, World!');\n    console.log('Batch edit works!');"
			},
			{
				"operation": "insert",
				"line_range": 6,
				"content": "// End of file"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();


		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		let expected = "# Header\n// Added by batch_edit\nfunction main() {\n    console.log('Hello, World!');\n    console.log('Batch edit works!');\n    return 0;\n}\n// Footer\n// End of file\n";
		assert_eq!(
			actual, expected,
			"Should handle comprehensive batch edit scenario"
		);
	}

	#[tokio::test]
	async fn test_batch_edit_with_undo_functionality() {

		let temp_file =
			create_test_file("original line 1\noriginal line 2\noriginal line 3\n").await;
		let path = temp_file.path().to_string_lossy().to_string();


		let operations = json!([
			{
				"operation": "insert",
				"line_range": 1,
				"content": "inserted after line 1"
			},
			{
				"operation": "replace",
				"line_range": [3, 3],
				"content": "replaced original line 3"
			}
		]);

		let batch_call = create_batch_edit_call(&path, operations).await;
		crate::mcp::fs::core::execute_batch_edit(&batch_call)
			.await
			.unwrap();


		let content_after_batch = fs::read_to_string(temp_file.path()).await.unwrap();
		let expected_after_batch =
			"original line 1\ninserted after line 1\noriginal line 2\nreplaced original line 3\n";
		assert_eq!(
			content_after_batch, expected_after_batch,
			"Content should reflect batch edit changes"
		);


		let undo_text = crate::mcp::fs::core::undo_edit(temp_file.path())
			.await
			.unwrap();


		let content_after_undo = fs::read_to_string(temp_file.path()).await.unwrap();
		let expected_original = "original line 1\noriginal line 2\noriginal line 3\n";
		assert_eq!(
			content_after_undo, expected_original,
			"Content should be restored to original after undo"
		);


		assert!(
			undo_text.contains("Successfully undid the last edit"),
			"Should contain undo confirmation message, got: {}",
			undo_text
		);
	}



	#[tokio::test]
	async fn test_batch_edit_insert_at_beginning() {

		let temp_file = create_test_file("line 1\nline 2\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([{"operation": "insert", "line_range": 0, "content": "line 0"}]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "line 0\nline 1\nline 2\n");
	}

	#[tokio::test]
	async fn test_batch_edit_insert_at_end() {

		let temp_file = create_test_file("line 1\nline 2\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([{"operation": "insert", "line_range": 2, "content": "line 3"}]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "line 1\nline 2\nline 3\n");
	}

	#[tokio::test]
	async fn test_batch_edit_insert_negative_index() {

		let temp_file = create_test_file("line 1\nline 2\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([{"operation": "insert", "line_range": -1, "content": "appended"}]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "line 1\nline 2\nappended\n");
	}

	#[tokio::test]
	async fn test_batch_edit_replace_out_of_bounds() {

		let temp_file = create_test_file("line 1\nline 2\nline 3\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([{"operation": "replace", "line_range": [99, 99], "content": "oops"}]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap_err();

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "line 1\nline 2\nline 3\n");
	}

	#[tokio::test]
	async fn test_batch_edit_replace_start_greater_than_end() {

		let temp_file = create_test_file("line 1\nline 2\nline 3\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([{"operation": "replace", "line_range": [3, 1], "content": "bad"}]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap_err();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "line 1\nline 2\nline 3\n");
	}

	#[tokio::test]
	async fn test_batch_edit_empty_operations_array() {

		let temp_file = create_test_file("line 1\nline 2\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(&path, json!([])).await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap_err();
	}

	#[tokio::test]
	async fn test_batch_edit_file_not_found() {

		let call = create_batch_edit_call(
			"/tmp/octofs_nonexistent_file_xyz_12345.txt",
			json!([{"operation": "insert", "line_range": 1, "content": "test"}]),
		)
		.await;
		let err = crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap_err();
		let msg = err.to_string();
		assert!(
			msg.contains("not found") || msg.contains("No such file"),
			"error should mention file not found: {}",
			msg
		);
	}

	#[tokio::test]
	async fn test_batch_edit_multiple_non_overlapping_replaces() {


		let temp_file = create_test_file("a\nb\nc\nd\ne\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [1, 1], "content": "A"},
				{"operation": "replace", "line_range": [3, 3], "content": "C"},
				{"operation": "replace", "line_range": [5, 5], "content": "E"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "A\nb\nC\nd\nE\n");
	}

	#[tokio::test]
	async fn test_batch_edit_replace_expand_lines() {

		let temp_file = create_test_file("before\nTARGET\nafter\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([{"operation": "replace", "line_range": [2, 2], "content": "new1\nnew2\nnew3"}]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "before\nnew1\nnew2\nnew3\nafter\n");
	}

	#[tokio::test]
	async fn test_batch_edit_replace_shrink_lines() {

		let temp_file = create_test_file("before\nA\nB\nC\nafter\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([{"operation": "replace", "line_range": [2, 4], "content": "SINGLE"}]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "before\nSINGLE\nafter\n");
	}

	#[tokio::test]
	async fn test_batch_edit_replace_empty_deletes_lines() {

		let temp_file = create_test_file("keep\ndelete me\nalso keep\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([{"operation": "replace", "line_range": [2, 2], "content": ""}]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "keep\nalso keep\n");
	}

	#[tokio::test]
	async fn test_batch_edit_replace_negative_line_range() {

		let temp_file = create_test_file("line 1\nline 2\nline 3\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([{"operation": "replace", "line_range": [-1, -1], "content": "LAST"}]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "line 1\nline 2\nLAST\n");
	}

	#[tokio::test]
	async fn test_batch_edit_concurrent_writes_atomicity() {


		let temp_file = create_test_file("line 1\nline 2\nline 3\n").await;
		let path = temp_file.path().to_string_lossy().to_string();



		let mut handles = Vec::new();
		for i in 0..10 {
			let p = path.clone();
			handles.push(tokio::spawn(async move {
				let call = McpToolCall {
					tool_id: format!("concurrent_{}", i),
					tool_name: "batch_edit".to_string(),
					parameters: json!({
						"path": p,
						"operations": [{"operation": "insert", "line_range": 0, "content": format!("marker_{}", i)}]
					}),
					workdir: std::env::current_dir().unwrap_or_default(),
				};
				crate::mcp::fs::core::execute_batch_edit(&call)
					.await
					.unwrap()
			}));
		}
		for handle in handles {

			handle.await.unwrap();
		}

		let final_content = fs::read_to_string(temp_file.path()).await.unwrap();
		assert!(
			final_content.contains("line 1"),
			"original content must survive concurrent writes"
		);
		assert!(
			final_content.contains("line 2"),
			"original content must survive concurrent writes"
		);
		assert!(
			final_content.contains("line 3"),
			"original content must survive concurrent writes"
		);
	}

	#[tokio::test]
	async fn test_lock_key_collapses_aliased_paths() {



		let dir = tempfile::tempdir().unwrap();
		let abs = dir.path().join("aliased.txt");
		fs::write(&abs, "x").await.unwrap();

		let raw = abs.clone();
		let with_dot = dir.path().join(".").join("aliased.txt");
		let with_double_slash =
			std::path::PathBuf::from(format!("{}//aliased.txt", dir.path().to_string_lossy()));

		let key_raw = crate::mcp::fs::text_editing::lock_key_for(&raw);
		let key_dot = crate::mcp::fs::text_editing::lock_key_for(&with_dot);
		let key_slash = crate::mcp::fs::text_editing::lock_key_for(&with_double_slash);

		assert_eq!(key_raw, key_dot, "`./x` must share a lock with `x`");
		assert_eq!(key_raw, key_slash, "`x//y` must share a lock with `x/y`");
	}

	#[tokio::test]
	async fn test_batch_edit_diff_shows_removed_and_added() {

		let temp_file = create_test_file("alpha\nbeta\ngamma\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([{"operation": "replace", "line_range": [2, 2], "content": "BETA_NEW"}]),
		)
		.await;
		let diff = crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		assert!(
			diff.contains("-2:"),
			"diff must show removed line: {}",
			diff
		);
		assert!(diff.contains("+2:"), "diff must show added line: {}", diff);
		assert!(
			diff.contains("beta"),
			"diff must show old content: {}",
			diff
		);
		assert!(
			diff.contains("BETA_NEW"),
			"diff must show new content: {}",
			diff
		);
	}

	#[tokio::test]
	async fn test_batch_edit_insert_and_replace_combined() {

		let temp_file = create_test_file("fn foo() {\n    old_body();\n}\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "insert", "line_range": 0, "content": "// generated"},
				{"operation": "replace", "line_range": [2, 2], "content": "    new_body();"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "// generated\nfn foo() {\n    new_body();\n}\n");
	}

	#[tokio::test]
	async fn test_batch_edit_preserves_no_trailing_newline() {

		let temp_file = create_test_file("line 1\nline 2").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([{"operation": "replace", "line_range": [1, 1], "content": "LINE ONE"}]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "LINE ONE\nline 2");
	}

	#[tokio::test]
	async fn test_batch_edit_replace_line_zero_is_invalid() {

		let temp_file = create_test_file("line 1\nline 2\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([{"operation": "replace", "line_range": [0, 1], "content": "bad"}]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap_err();

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "line 1\nline 2\n");
	}



	#[tokio::test]
	async fn test_batch_edit_replace_1_with_10_then_replace_later_line() {


		let temp_file = create_test_file("L1\nL2\nL3\nL4\nL5\nL6\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [2, 2], "content": "N1\nN2\nN3\nN4\nN5\nN6\nN7\nN8\nN9\nN10"},
				{"operation": "replace", "line_range": [5, 5], "content": "REPLACED_L5"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();

		assert_eq!(
			actual,
			"L1\nN1\nN2\nN3\nN4\nN5\nN6\nN7\nN8\nN9\nN10\nL3\nL4\nREPLACED_L5\nL6\n"
		);
	}

	#[tokio::test]
	async fn test_batch_edit_shrink_3_to_1_then_replace_later_line() {


		let temp_file = create_test_file("L1\nL2\nL3\nL4\nL5\nL6\nL7\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [2, 4], "content": "MERGED"},
				{"operation": "replace", "line_range": [6, 6], "content": "REPLACED_L6"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "L1\nMERGED\nL5\nREPLACED_L6\nL7\n");
	}

	#[tokio::test]
	async fn test_batch_edit_inserts_and_replaces_mixed() {

		let temp_file = create_test_file("A\nB\nC\nD\nE\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "insert", "line_range": 0, "content": "HEADER"},
				{"operation": "replace", "line_range": [3, 3], "content": "C_NEW"},
				{"operation": "insert", "line_range": 5, "content": "FOOTER"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "HEADER\nA\nB\nC_NEW\nD\nE\nFOOTER\n");
	}

	#[tokio::test]
	async fn test_batch_edit_insert_into_empty_file() {

		let temp_file = create_test_file("").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([{"operation": "insert", "line_range": 0, "content": "first line\nsecond line"}]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "first line\nsecond line");
	}

	#[tokio::test]
	async fn test_batch_edit_replace_all_lines_with_different_count() {

		let temp_file = create_test_file("old1\nold2\nold3\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([{"operation": "replace", "line_range": [1, 3], "content": "new1\nnew2\nnew3\nnew4\nnew5"}]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "new1\nnew2\nnew3\nnew4\nnew5\n");
	}

	#[tokio::test]
	async fn test_batch_edit_three_replaces_expand_shrink_same() {

		let temp_file = create_test_file("L1\nL2\nL3\nL4\nL5\nL6\nL7\nL8\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [1, 1], "content": "X1\nX2\nX3"},
				{"operation": "replace", "line_range": [4, 5], "content": "MERGED_45"},
				{"operation": "replace", "line_range": [7, 7], "content": "SAME_7"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "X1\nX2\nX3\nL2\nL3\nMERGED_45\nL6\nSAME_7\nL8\n");
	}

	#[tokio::test]
	async fn test_batch_edit_insert_after_last_and_replace_first() {

		let temp_file = create_test_file("first\nmiddle\nlast\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "insert", "line_range": -1, "content": "appended"},
				{"operation": "replace", "line_range": [1, 1], "content": "FIRST"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "FIRST\nmiddle\nlast\nappended\n");
	}



	#[tokio::test]
	async fn test_batch_edit_content_null_gives_clear_error() {

		let temp_file = create_test_file("line 1\nline 2\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([{"operation": "insert", "line_range": 1, "content": null}]),
		)
		.await;


		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap_err();

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "line 1\nline 2\n");
	}

	#[tokio::test]
	async fn test_batch_edit_missing_operation_field() {

		let temp_file = create_test_file("line 1\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call =
			create_batch_edit_call(&path, json!([{"line_range": 1, "content": "test"}])).await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap_err();
	}

	#[tokio::test]
	async fn test_batch_edit_unsupported_operation_type() {

		let temp_file = create_test_file("line 1\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([{"operation": "delete", "line_range": 1, "content": "test"}]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap_err();
	}

	#[tokio::test]
	async fn test_batch_edit_max_operations_exceeded() {

		let temp_file = create_test_file("line 1\nline 2\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let ops: Vec<serde_json::Value> = (0..51)
			.map(
				|i| json!({"operation": "insert", "line_range": 0, "content": format!("op_{}", i)}),
			)
			.collect();
		let call = create_batch_edit_call(&path, json!(ops)).await;
		let err = crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap_err();
		let msg = err.to_string();
		assert!(
			msg.contains("Too many operations"),
			"error should mention too many operations: {}",
			msg
		);

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "line 1\nline 2\n");
	}

	#[tokio::test]
	async fn test_batch_edit_large_file_scattered_operations() {

		let mut content = String::new();
		for i in 1..=1000 {
			content.push_str(&format!("line {}\n", i));
		}
		let temp_file = create_test_file(&content).await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [1, 1], "content": "FIRST"},
				{"operation": "replace", "line_range": [250, 250], "content": "LINE_250"},
				{"operation": "replace", "line_range": [500, 500], "content": "LINE_500"},
				{"operation": "replace", "line_range": [750, 750], "content": "LINE_750"},
				{"operation": "replace", "line_range": [1000, 1000], "content": "LAST"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		let lines: Vec<&str> = actual.lines().collect();
		assert_eq!(lines[0], "FIRST");
		assert_eq!(lines[1], "line 2");
		assert_eq!(lines[249], "LINE_250");
		assert_eq!(lines[499], "LINE_500");
		assert_eq!(lines[749], "LINE_750");
		assert_eq!(lines[999], "LAST");
		assert_eq!(lines.len(), 1000);
	}

	#[tokio::test]
	async fn test_batch_edit_expand_early_shrink_late_verify_all_lines() {

		let temp_file = create_test_file("L1\nL2\nL3\nL4\nL5\nL6\nL7\nL8\nL9\nL10\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [1, 1], "content": "E1\nE2\nE3\nE4\nE5"},
				{"operation": "replace", "line_range": [8, 10], "content": "SHRUNK"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(
			actual,
			"E1\nE2\nE3\nE4\nE5\nL2\nL3\nL4\nL5\nL6\nL7\nSHRUNK\n"
		);
	}

	#[tokio::test]
	async fn test_batch_edit_replace_single_line_range_as_integer() {


		let temp_file = create_test_file("A\nB\nC\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{"operation": "replace", "line_range": 2, "content": "B_NEW"}]
			}),
		};

		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "A\nB_NEW\nC\n");
	}






	#[tokio::test]
	async fn test_batch_edit_insert_adjacent_lines_no_conflict() {

		let temp_file = create_test_file("A\nB\nC\nD\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "insert", "line_range": 1, "content": "after_A"},
				{"operation": "insert", "line_range": 3, "content": "after_C"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "A\nafter_A\nB\nC\nafter_C\nD\n");
	}

	#[tokio::test]
	async fn test_batch_edit_insert_after_line_and_replace_next_line() {

		let temp_file = create_test_file("A\nB\nC\nD\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "insert", "line_range": 2, "content": "INSERTED"},
				{"operation": "replace", "line_range": [3, 3], "content": "C_NEW"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();

		assert_eq!(actual, "A\nB\nINSERTED\nC_NEW\nD\n");
	}

	#[tokio::test]
	async fn test_batch_edit_expand_replace_then_insert_before() {


		let temp_file = create_test_file("L1\nL2\nL3\nL4\nL5\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [4, 4], "content": "X1\nX2\nX3\nX4\nX5"},
				{"operation": "insert", "line_range": 1, "content": "HEADER"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "L1\nHEADER\nL2\nL3\nX1\nX2\nX3\nX4\nX5\nL5\n");
	}

	#[tokio::test]
	async fn test_batch_edit_delete_and_insert_nearby() {

		let temp_file = create_test_file("A\nB\nC\nD\nE\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [3, 3], "content": ""},
				{"operation": "insert", "line_range": 1, "content": "NEW"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();

		assert_eq!(actual, "A\nNEW\nB\nD\nE\n");
	}

	#[tokio::test]
	async fn test_batch_edit_two_inserts_same_line_conflicts() {

		let temp_file = create_test_file("A\nB\nC\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "insert", "line_range": 2, "content": "first"},
				{"operation": "insert", "line_range": 2, "content": "second"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap_err();

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "A\nB\nC\n");
	}

	#[tokio::test]
	async fn test_batch_edit_replace_first_and_last_line() {

		let temp_file = create_test_file("FIRST\nM1\nM2\nM3\nLAST\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [1, 1], "content": "NEW_FIRST"},
				{"operation": "replace", "line_range": [5, 5], "content": "NEW_LAST"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "NEW_FIRST\nM1\nM2\nM3\nNEW_LAST\n");
	}

	#[tokio::test]
	async fn test_batch_edit_multiline_insert_with_expand_replace() {

		let temp_file = create_test_file("A\nB\nC\nD\nE\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "insert", "line_range": 0, "content": "H1\nH2\nH3"},
				{"operation": "replace", "line_range": [3, 3], "content": "C1\nC2\nC3\nC4"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "H1\nH2\nH3\nA\nB\nC1\nC2\nC3\nC4\nD\nE\n");
	}

	#[tokio::test]
	async fn test_batch_edit_four_ops_insert_replace_delete_insert() {

		let temp_file = create_test_file("L1\nL2\nL3\nL4\nL5\nL6\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "insert", "line_range": 0, "content": "HEADER"},
				{"operation": "replace", "line_range": [2, 2], "content": "L2_NEW"},
				{"operation": "replace", "line_range": [4, 4], "content": ""},
				{"operation": "insert", "line_range": 6, "content": "FOOTER"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();

		assert_eq!(actual, "HEADER\nL1\nL2_NEW\nL3\nL5\nL6\nFOOTER\n");
	}

	#[tokio::test]
	async fn test_batch_edit_expand_early_insert_late_verify_positions() {


		let temp_file = create_test_file("L1\nL2\nL3\nL4\nL5\nL6\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [1, 1], "content": "R1\nR2\nR3\nR4"},
				{"operation": "insert", "line_range": 5, "content": "AFTER_L5"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();

		assert_eq!(actual, "R1\nR2\nR3\nR4\nL2\nL3\nL4\nL5\nAFTER_L5\nL6\n");
	}

	#[tokio::test]
	async fn test_batch_edit_shrink_early_insert_late_verify_positions() {

		let temp_file = create_test_file("L1\nL2\nL3\nL4\nL5\nL6\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [1, 3], "content": "MERGED"},
				{"operation": "insert", "line_range": 5, "content": "AFTER_L5"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();

		assert_eq!(actual, "MERGED\nL4\nL5\nAFTER_L5\nL6\n");
	}

	#[tokio::test]
	async fn test_batch_edit_ops_given_in_reverse_order() {


		let temp_file = create_test_file("A\nB\nC\nD\nE\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "insert", "line_range": 5, "content": "FOOTER"},
				{"operation": "replace", "line_range": [3, 3], "content": "C_NEW"},
				{"operation": "insert", "line_range": 0, "content": "HEADER"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "HEADER\nA\nB\nC_NEW\nD\nE\nFOOTER\n");
	}

	#[tokio::test]
	async fn test_batch_edit_real_world_two_replaces_different_sizes() {



		let mut content = String::new();
		for i in 1..=50 {
			content.push_str(&format!("line {}\n", i));
		}
		let temp_file = create_test_file(&content).await;
		let path = temp_file.path().to_string_lossy().to_string();



		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [10, 15], "content": "R1\nR2\nR3\nR4\nR5\nR6\nR7\nR8\nR9\nR10"},
				{"operation": "replace", "line_range": [30, 35], "content": "S1\nS2\nS3"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		let lines: Vec<&str> = actual.lines().collect();


		for (i, line) in lines.iter().take(9).enumerate() {
			assert_eq!(
				*line,
				format!("line {}", i + 1),
				"line {} should be unchanged",
				i + 1
			);
		}

		for i in 0..10 {
			assert_eq!(lines[9 + i], format!("R{}", i + 1));
		}

		for i in 16..=29 {
			assert_eq!(lines[9 + 10 + (i - 16)], format!("line {}", i));
		}

		let s_start = 9 + 10 + 14;
		assert_eq!(lines[s_start], "S1");
		assert_eq!(lines[s_start + 1], "S2");
		assert_eq!(lines[s_start + 2], "S3");

		for i in 36..=50 {
			let pos = s_start + 3 + (i - 36);
			assert_eq!(lines[pos], format!("line {}", i));
		}

		assert_eq!(lines.len(), 51);
	}

	#[tokio::test]
	async fn test_batch_edit_five_scattered_inserts() {

		let temp_file = create_test_file("L1\nL2\nL3\nL4\nL5\nL6\nL7\nL8\nL9\nL10\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "insert", "line_range": 0, "content": "I0"},
				{"operation": "insert", "line_range": 2, "content": "I2"},
				{"operation": "insert", "line_range": 5, "content": "I5"},
				{"operation": "insert", "line_range": 8, "content": "I8"},
				{"operation": "insert", "line_range": 10, "content": "I10"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(
			actual,
			"I0\nL1\nL2\nI2\nL3\nL4\nL5\nI5\nL6\nL7\nL8\nI8\nL9\nL10\nI10\n"
		);
	}

	#[tokio::test]
	async fn test_batch_edit_error_does_not_modify_file() {


		let temp_file = create_test_file("A\nB\nC\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [1, 1], "content": "A_NEW"},
				{"operation": "replace", "line_range": [99, 99], "content": "INVALID"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap_err();

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "A\nB\nC\n");
	}

	#[tokio::test]
	async fn test_batch_edit_multiline_insert_between_two_replaces() {

		let temp_file = create_test_file("L1\nL2\nL3\nL4\nL5\nL6\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [1, 1], "content": "FIRST"},
				{"operation": "insert", "line_range": 3, "content": "I1\nI2\nI3"},
				{"operation": "replace", "line_range": [5, 5], "content": "FIFTH"}
			]),
		)
		.await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "FIRST\nL2\nL3\nI1\nI2\nI3\nL4\nFIFTH\nL6\n");
	}


	#[tokio::test]
	async fn test_text_editor_view_negative_indexing() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let file_path = temp_dir.path().join("test.txt");


		fs::write(&file_path, "line 1\nline 2\nline 3\nline 4\nline 5\n")
			.await
			.unwrap();


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": file_path.to_string_lossy(),
				"lines": [-1, -1]
			}),
		};

		let content = execute_view(&call).await.unwrap();
		assert!(
			content.contains("5:line 5"),
			"Should show last line: {}",
			content
		);


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": file_path.to_string_lossy(),
				"lines": [-2, -2]
			}),
		};

		let content = execute_view(&call).await.unwrap();
		assert!(
			content.contains("4:line 4"),
			"Should show second-to-last line: {}",
			content
		);


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": file_path.to_string_lossy(),
				"lines": [-3, -1]
			}),
		};

		let content = execute_view(&call).await.unwrap();
		assert!(
			content.contains("3:line 3"),
			"Should show line 3: {}",
			content
		);
		assert!(
			content.contains("4:line 4"),
			"Should show line 4: {}",
			content
		);
		assert!(
			content.contains("5:line 5"),
			"Should show line 5: {}",
			content
		);


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": file_path.to_string_lossy(),
				"lines": [2, -2]
			}),
		};

		let content = execute_view(&call).await.unwrap();
		assert!(
			content.contains("2:line 2"),
			"Should show line 2: {}",
			content
		);
		assert!(
			content.contains("3:line 3"),
			"Should show line 3: {}",
			content
		);
		assert!(
			content.contains("4:line 4"),
			"Should show line 4: {}",
			content
		);
	}

	#[tokio::test]
	async fn test_text_editor_view_negative_indexing_clamps_out_of_bounds() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let file_path = temp_dir.path().join("test.txt");


		fs::write(&file_path, "line 1\nline 2\nline 3\n")
			.await
			.unwrap();


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": file_path.to_string_lossy(),
				"lines": [-5, -1]
			}),
		};

		let content = execute_view(&call).await.expect("should clamp, not error");
		assert!(
			content.contains("1:line 1")
				&& content.contains("2:line 2")
				&& content.contains("3:line 3"),
			"Should return all 3 lines after clamp: {content}"
		);
	}

	#[tokio::test]
	async fn test_extract_lines_negative_indexing() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let source_path = temp_dir.path().join("source.txt");
		let target_path = temp_dir.path().join("target.txt");


		fs::write(&source_path, "line 1\nline 2\nline 3\nline 4\nline 5\n")
			.await
			.unwrap();
		fs::write(&target_path, "").await.unwrap();


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "extract_lines".to_string(),
			parameters: json!({
				"from_path": source_path.to_string_lossy(),
				"from_range": [-1, -1],
				"append_path": target_path.to_string_lossy(),
				"append_line": -1
			}),
		};

		execute_extract_lines(&call).await.unwrap();

		let target_content = fs::read_to_string(&target_path).await.unwrap();
		assert_eq!(target_content.trim(), "line 5", "Should extract last line");


		fs::write(&target_path, "").await.unwrap();
		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "extract_lines".to_string(),
			parameters: json!({
				"from_path": source_path.to_string_lossy(),
				"from_range": [-2, -1],
				"append_path": target_path.to_string_lossy(),
				"append_line": -1
			}),
		};

		execute_extract_lines(&call).await.unwrap();

		let target_content = fs::read_to_string(&target_path).await.unwrap();
		assert_eq!(
			target_content.trim(),
			"line 4\nline 5",
			"Should extract last 2 lines"
		);


		fs::write(&target_path, "").await.unwrap();
		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "extract_lines".to_string(),
			parameters: json!({
				"from_path": source_path.to_string_lossy(),
				"from_range": [2, -2],
				"append_path": target_path.to_string_lossy(),
				"append_line": -1
			}),
		};

		execute_extract_lines(&call).await.unwrap();

		let target_content = fs::read_to_string(&target_path).await.unwrap();
		assert_eq!(
			target_content.trim(),
			"line 2\nline 3\nline 4",
			"Should extract lines 2-4"
		);
	}

	#[tokio::test]
	async fn test_extract_lines_negative_indexing_errors() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let source_path = temp_dir.path().join("source.txt");
		let target_path = temp_dir.path().join("target.txt");


		fs::write(&source_path, "line 1\nline 2\nline 3\n")
			.await
			.unwrap();
		fs::write(&target_path, "").await.unwrap();


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "extract_lines".to_string(),
			parameters: json!({
				"from_path": source_path.to_string_lossy(),
				"from_range": [-5, -1],
				"append_path": target_path.to_string_lossy(),
				"append_line": -1
			}),
		};

		let err = execute_extract_lines(&call).await.unwrap_err();
		let content = err.to_string();
		assert!(
			content.contains("exceeds file length"),
			"Should show error: {}",
			content
		);
	}

	#[tokio::test]
	async fn test_batch_edit_negative_indexing() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let file_path = temp_dir.path().join("test.txt");


		fs::write(&file_path, "line 1\nline 2\nline 3\nline 4\nline 5\n")
			.await
			.unwrap();


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": file_path.to_string_lossy(),
				"operations": [
					{
						"operation": "replace",
						"line_range": [-1, -1],
						"content": "LAST LINE REPLACED"
					}
				]
			}),
		};

		execute_batch_edit(&call).await.unwrap();

		let content = fs::read_to_string(&file_path).await.unwrap();
		assert!(
			content.contains("LAST LINE REPLACED"),
			"Should replace last line: {}",
			content
		);
		assert!(
			!content.contains("line 5"),
			"Should not contain original last line"
		);


		fs::write(&file_path, "line 1\nline 2\nline 3\nline 4\nline 5\n")
			.await
			.unwrap();
		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": file_path.to_string_lossy(),
				"operations": [
					{
						"operation": "replace",
						"line_range": [-2, -1],
						"content": "REPLACED LINES 4-5"
					}
				]
			}),
		};

		execute_batch_edit(&call).await.unwrap();

		let content = fs::read_to_string(&file_path).await.unwrap();
		assert!(
			content.contains("REPLACED LINES 4-5"),
			"Should replace last 2 lines: {}",
			content
		);
		assert!(
			!content.contains("line 4"),
			"Should not contain original line 4"
		);
		assert!(
			!content.contains("line 5"),
			"Should not contain original line 5"
		);


		fs::write(&file_path, "line 1\nline 2\nline 3\nline 4\nline 5\n")
			.await
			.unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": file_path.to_string_lossy(),
				"operations": [
					{
						"operation": "insert",
						"line_range": -2,
						"content": "INSERTED AFTER LINE 4"
					}
				]
			}),
		};

		execute_batch_edit(&call).await.unwrap();

		let content = fs::read_to_string(&file_path).await.unwrap();
		let lines: Vec<&str> = content.lines().collect();

		assert_eq!(
			lines[4], "INSERTED AFTER LINE 4",
			"Should insert after line 4"
		);
		assert_eq!(lines[5], "line 5", "Line 5 should be moved down");
	}

	#[tokio::test]
	async fn test_batch_edit_negative_indexing_errors() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let file_path = temp_dir.path().join("test.txt");


		fs::write(&file_path, "line 1\nline 2\nline 3\n")
			.await
			.unwrap();


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": file_path.to_string_lossy(),
				"operations": [
					{
						"operation": "replace",
						"line_range": [-5, -1],
						"content": "SHOULD FAIL"
					}
				]
			}),
		};

		let err = execute_batch_edit(&call).await.unwrap_err();
		let content = err.to_string();
		assert!(
			content.contains("exceeds file length"),
			"Should show error: {}",
			content
		);
	}

	#[tokio::test]
	async fn test_negative_indexing_edge_cases() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let file_path = temp_dir.path().join("test.txt");


		fs::write(&file_path, "only line\n").await.unwrap();


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": file_path.to_string_lossy(),
				"lines": [-1, -1]
			}),
		};

		let content = execute_view(&call).await.unwrap();
		assert!(
			content.contains("1:only line"),
			"Should show the only line: {}",
			content
		);


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": file_path.to_string_lossy(),
				"lines": [-2, -1]
			}),
		};

		let content = execute_view(&call).await.expect("should clamp, not error");
		assert!(
			content.contains("1:only line"),
			"Should clamp and return the only line: {content}"
		);
	}



	#[tokio::test]
	async fn test_view_directory_lists_files() {

		let temp_dir = tempfile::TempDir::new().unwrap();
		fs::write(temp_dir.path().join("alpha.rs"), "fn a() {}")
			.await
			.unwrap();
		fs::write(temp_dir.path().join("beta.rs"), "fn b() {}")
			.await
			.unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({ "paths": temp_dir.path().to_string_lossy() }),
		};

		let output = execute_view(&call).await.unwrap();
		assert!(
			output.contains("alpha.rs") || output.contains("beta.rs"),
			"Should list files: {output}"
		);
	}

	#[tokio::test]
	async fn test_view_directory_content_search() {

		let temp_dir = tempfile::TempDir::new().unwrap();
		fs::write(temp_dir.path().join("foo.rs"), "fn hello_world() {}")
			.await
			.unwrap();
		fs::write(temp_dir.path().join("bar.rs"), "fn unrelated() {}")
			.await
			.unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": temp_dir.path().to_string_lossy(),
				"content": "hello_world"
			}),
		};

		let output = execute_view(&call).await.unwrap();
		assert!(
			output.contains("hello_world"),
			"Should find match: {output}"
		);
	}

	#[tokio::test]
	async fn test_view_directory_pattern_filter() {

		let temp_dir = tempfile::TempDir::new().unwrap();
		fs::write(temp_dir.path().join("main.rs"), "fn main() {}")
			.await
			.unwrap();
		fs::write(temp_dir.path().join("config.toml"), "[package]")
			.await
			.unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": temp_dir.path().to_string_lossy(),
				"pattern": "*.toml"
			}),
		};

		let output = execute_view(&call).await.unwrap();

		assert!(
			output.contains("config.toml"),
			"Should list config.toml: {}",
			output
		);
	}

	#[tokio::test]
	async fn test_view_file_path_reads_content() {

		let temp_dir = tempfile::TempDir::new().unwrap();
		let file_path = temp_dir.path().join("hello.txt");
		fs::write(&file_path, "line one\nline two\n").await.unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({ "paths": file_path.to_string_lossy() }),
		};

		let content = execute_view(&call).await.unwrap();
		assert!(
			content.contains("1:line one"),
			"Should show line 1: {content}"
		);
		assert!(
			content.contains("2:line two"),
			"Should show line 2: {content}"
		);
	}

	#[tokio::test]
	async fn test_view_missing_path_errors() {

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({}),
		};

		let err = execute_view(&call).await.unwrap_err();
		let msg = err.to_string();
		assert!(msg.contains("paths"), "Error should mention 'paths': {msg}");
	}

	#[tokio::test]
	async fn test_batch_edit_four_operations_original_line_numbers() {


		let temp_file = create_test_file(
			"line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\n",
		)
		.await;
		let path = temp_file.path().to_string_lossy().to_string();

		let operations = json!([
			{
				"operation": "replace",
				"line_range": [2, 2],
				"content": "REPLACED LINE 2"
			},
			{
				"operation": "insert",
				"line_range": 4,
				"content": "INSERTED AFTER ORIGINAL LINE 4"
			},
			{
				"operation": "replace",
				"line_range": [6, 7],
				"content": "REPLACED ORIGINAL LINES 6-7"
			},
			{
				"operation": "insert",
				"line_range": 9,
				"content": "INSERTED AFTER ORIGINAL LINE 9"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();


		let actual = fs::read_to_string(temp_file.path()).await.unwrap();






		let expected = "line 1\nREPLACED LINE 2\nline 3\nline 4\nINSERTED AFTER ORIGINAL LINE 4\nline 5\nREPLACED ORIGINAL LINES 6-7\nline 8\nline 9\nINSERTED AFTER ORIGINAL LINE 9\nline 10\n";

		assert_eq!(
			actual, expected,
			"Content should reflect all 4 operations using original line numbers.\nActual:\n{}\nExpected:\n{}",
			actual, expected
		);
	}

	#[tokio::test]
	async fn test_batch_edit_overlapping_operations_should_fail() {


		let temp_file = create_test_file("line 1\nline 2\nline 3\nline 4\nline 5\n").await;
		let path = temp_file.path().to_string_lossy().to_string();


		let operations = json!([
			{
				"operation": "replace",
				"line_range": [1, 3],
				"content": "REPLACED 1-3"
			},
			{
				"operation": "replace",
				"line_range": [3, 5],
				"content": "REPLACED 3-5"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		let err = crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap_err();

		let content = err.to_string();
		assert!(
			content.contains("Conflicting operations"),
			"Should detect conflict: {}",
			content
		);
	}

	#[tokio::test]
	async fn test_batch_edit_insert_and_replace_same_line_succeeds() {


		let temp_file = create_test_file("line 1\nline 2\nline 3\nline 4\nline 5\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let operations = json!([
			{
				"operation": "insert",
				"line_range": 2,
				"content": "INSERTED AFTER 2"
			},
			{
				"operation": "replace",
				"line_range": [2, 2],
				"content": "REPLACED 2"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();


		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(
			actual,
			"line 1\nREPLACED 2\nINSERTED AFTER 2\nline 3\nline 4\nline 5\n"
		);
	}

	#[tokio::test]
	async fn test_batch_edit_expansion_operations_atomic() {


		let temp_file = create_test_file("line 1\nline 2\nline 3\nline 4\nline 5\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let operations = json!([
			{
				"operation": "replace",
				"line_range": [1, 1],
				"content": "NEW LINE 1A\nNEW LINE 1B\nNEW LINE 1C\nNEW LINE 1D"
			},
			{
				"operation": "replace",
				"line_range": [5, 5],
				"content": "NEW LINE 5A\nNEW LINE 5B\nNEW LINE 5C"
			},
			{
				"operation": "insert",
				"line_range": 3,
				"content": "INSERTED AFTER ORIGINAL 3"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();


		let actual = fs::read_to_string(temp_file.path()).await.unwrap();


		let expected = "NEW LINE 1A\nNEW LINE 1B\nNEW LINE 1C\nNEW LINE 1D\nline 2\nline 3\nINSERTED AFTER ORIGINAL 3\nline 4\nNEW LINE 5A\nNEW LINE 5B\nNEW LINE 5C\n";

		assert_eq!(
			actual, expected,
			"Content should reflect atomic operations on original positions.\nActual:\n{}\nExpected:\n{}",
			actual, expected
		);
	}

	#[tokio::test]
	async fn test_batch_edit_complex_mixed_operations() {

		let temp_file = create_test_file("A\nB\nC\nD\nE\nF\nG\nH\nI\nJ\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let operations = json!([
			{
				"operation": "insert",
				"line_range": 1,
				"content": "AFTER_A"
			},
			{
				"operation": "replace",
				"line_range": [2, 4],
				"content": "BCD_REPLACED"
			},
			{
				"operation": "insert",
				"line_range": 6,
				"content": "AFTER_F"
			},
			{
				"operation": "replace",
				"line_range": [8, 8],
				"content": "H1\nH2\nH3"
			},
			{
				"operation": "insert",
				"line_range": 10,
				"content": "FOOTER"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();


		let actual = fs::read_to_string(temp_file.path()).await.unwrap();













		let expected = "A\nAFTER_A\nBCD_REPLACED\nE\nF\nAFTER_F\nG\nH1\nH2\nH3\nI\nJ\nFOOTER\n";

		assert_eq!(
			actual, expected,
			"Complex mixed operations should work atomically.\nActual:\n{}\nExpected:\n{}",
			actual, expected
		);
	}

	#[tokio::test]
	async fn test_batch_edit_edge_case_adjacent_operations() {

		let temp_file = create_test_file("line 1\nline 2\nline 3\nline 4\nline 5\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let operations = json!([
			{
				"operation": "replace",
				"line_range": [1, 1],
				"content": "REPLACED 1"
			},
			{
				"operation": "replace",
				"line_range": [2, 2],
				"content": "REPLACED 2"
			},
			{
				"operation": "insert",
				"line_range": 3,
				"content": "AFTER 3"
			},
			{
				"operation": "replace",
				"line_range": [4, 4],
				"content": "REPLACED 4"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		let expected = "REPLACED 1\nREPLACED 2\nline 3\nAFTER 3\nREPLACED 4\nline 5\n";

		assert_eq!(
			actual, expected,
			"Adjacent operations should work correctly.\nActual:\n{}\nExpected:\n{}",
			actual, expected
		);
	}

	#[tokio::test]
	async fn test_batch_edit_your_exact_scenario_should_fail() {


		let temp_file = create_test_file("line 1\nline 2\nline 3\nline 4\nline 5\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let operations = json!([
			{
				"operation": "replace",
				"line_range": [1, 1],
				"content": "NEW1A\nNEW1B\nNEW1C\nNEW1D"
			},
			{
				"operation": "replace",
				"line_range": [3, 3],
				"content": "NEW3A\nNEW3B\nNEW3C\nNEW3D"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();

		let expected =
			"NEW1A\nNEW1B\nNEW1C\nNEW1D\nline 2\nNEW3A\nNEW3B\nNEW3C\nNEW3D\nline 4\nline 5\n";

		assert_eq!(
			actual, expected,
			"Your scenario should work when lines don't overlap.\nActual:\n{}\nExpected:\n{}",
			actual, expected
		);
	}

	#[tokio::test]
	async fn test_batch_edit_overlapping_ranges_should_fail() {

		let temp_file = create_test_file("line 1\nline 2\nline 3\nline 4\nline 5\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let operations = json!([
			{
				"operation": "replace",
				"line_range": [1, 3],
				"content": "REPLACED_1_TO_3"
			},
			{
				"operation": "replace",
				"line_range": [3, 5],
				"content": "REPLACED_3_TO_5"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		let err = crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap_err();
		let content = err.to_string();
		assert!(
			content.contains("Conflicting operations"),
			"Should detect overlap at line 3: {}",
			content
		);
	}

	#[tokio::test]
	async fn test_batch_edit_ultimate_stress_test() {


		let temp_file = create_test_file("A\nB\nC\nD\nE\nF\nG\nH\nI\nJ\nK\nL\nM\nN\nO\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let operations = json!([
			{
				"operation": "replace",
				"line_range": [1, 1],
				"content": "A1\nA2\nA3"
			},
			{
				"operation": "replace",
				"line_range": [3, 3],
				"content": "C1\nC2\nC3\nC4\nC5"
			},
			{
				"operation": "insert",
				"line_range": 5,
				"content": "AFTER_E1\nAFTER_E2"
			},
			{
				"operation": "replace",
				"line_range": [7, 9],
				"content": "GHI_1\nGHI_2"
			},
			{
				"operation": "insert",
				"line_range": 12,
				"content": "AFTER_L"
			},
			{
				"operation": "replace",
				"line_range": [15, 15],
				"content": "O1\nO2\nO3\nO4"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();















		let expected = "A1\nA2\nA3\nB\nC1\nC2\nC3\nC4\nC5\nD\nE\nAFTER_E1\nAFTER_E2\nF\nGHI_1\nGHI_2\nJ\nK\nL\nAFTER_L\nM\nN\nO1\nO2\nO3\nO4\n";

		assert_eq!(
			actual, expected,
			"Ultimate stress test with expansions should work atomically.\nActual:\n{}\nExpected:\n{}",
			actual, expected
		);
	}

	#[tokio::test]
	async fn test_batch_edit_extreme_expansions_and_contractions() {


		let temp_file = create_test_file("L1\nL2\nL3\nL4\nL5\nL6\nL7\nL8\nL9\nL10\nL11\nL12\nL13\nL14\nL15\nL16\nL17\nL18\nL19\nL20\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let operations = json!([
			{
				"operation": "replace",
				"line_range": [1, 1],
				"content": "EXP1_1\nEXP1_2\nEXP1_3\nEXP1_4\nEXP1_5\nEXP1_6\nEXP1_7\nEXP1_8\nEXP1_9\nEXP1_10"
			},
			{
				"operation": "replace",
				"line_range": [3, 7],
				"content": "CONTRACTED_3_TO_7"
			},
			{
				"operation": "replace",
				"line_range": [9, 9],
				"content": "EXP9_1\nEXP9_2\nEXP9_3\nEXP9_4\nEXP9_5\nEXP9_6\nEXP9_7\nEXP9_8"
			},
			{
				"operation": "replace",
				"line_range": [12, 16],
				"content": "CONTRACT_12_16_A\nCONTRACT_12_16_B"
			},
			{
				"operation": "insert",
				"line_range": 18,
				"content": "INS18_1\nINS18_2\nINS18_3\nINS18_4\nINS18_5\nINS18_6"
			},
			{
				"operation": "replace",
				"line_range": [20, 20],
				"content": "EXP20_1\nEXP20_2\nEXP20_3\nEXP20_4\nEXP20_5\nEXP20_6\nEXP20_7\nEXP20_8\nEXP20_9\nEXP20_10\nEXP20_11\nEXP20_12"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();














		let expected = "EXP1_1\nEXP1_2\nEXP1_3\nEXP1_4\nEXP1_5\nEXP1_6\nEXP1_7\nEXP1_8\nEXP1_9\nEXP1_10\nL2\nCONTRACTED_3_TO_7\nL8\nEXP9_1\nEXP9_2\nEXP9_3\nEXP9_4\nEXP9_5\nEXP9_6\nEXP9_7\nEXP9_8\nL10\nL11\nCONTRACT_12_16_A\nCONTRACT_12_16_B\nL17\nL18\nINS18_1\nINS18_2\nINS18_3\nINS18_4\nINS18_5\nINS18_6\nL19\nEXP20_1\nEXP20_2\nEXP20_3\nEXP20_4\nEXP20_5\nEXP20_6\nEXP20_7\nEXP20_8\nEXP20_9\nEXP20_10\nEXP20_11\nEXP20_12\n";

		assert_eq!(
			actual, expected,
			"CRITICAL: Extreme expansions/contractions must use original line positions!\nActual:\n{}\nExpected:\n{}",
			actual, expected
		);
	}

	#[tokio::test]
	async fn test_batch_edit_massive_file_with_extreme_operations() {

		let mut content = String::new();
		for i in 1..=50 {
			content.push_str(&format!("LINE_{:02}\n", i));
		}
		let temp_file = create_test_file(&content).await;
		let path = temp_file.path().to_string_lossy().to_string();

		let operations = json!([
			{
				"operation": "replace",
				"line_range": [5, 5],
				"content": "E5_01\nE5_02\nE5_03\nE5_04\nE5_05\nE5_06\nE5_07\nE5_08\nE5_09\nE5_10\nE5_11\nE5_12\nE5_13\nE5_14\nE5_15"
			},
			{
				"operation": "replace",
				"line_range": [10, 20],
				"content": "MEGA_CONTRACTION_10_TO_20"
			},
			{
				"operation": "insert",
				"line_range": 25,
				"content": "I25_1\nI25_2\nI25_3\nI25_4\nI25_5\nI25_6\nI25_7\nI25_8"
			},
			{
				"operation": "replace",
				"line_range": [30, 35],
				"content": "M30_01\nM30_02\nM30_03\nM30_04\nM30_05\nM30_06\nM30_07\nM30_08\nM30_09\nM30_10\nM30_11\nM30_12\nM30_13\nM30_14\nM30_15\nM30_16\nM30_17\nM30_18\nM30_19\nM30_20"
			},
			{
				"operation": "replace",
				"line_range": [40, 49],
				"content": "BIG_CONTRACT_A\nBIG_CONTRACT_B"
			},
			{
				"operation": "insert",
				"line_range": 50,
				"content": "FINAL_1\nFINAL_2\nFINAL_3\nFINAL_4\nFINAL_5"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();


		let mut expected_lines = Vec::new();


		for i in 1..=4 {
			expected_lines.push(format!("LINE_{:02}", i));
		}


		for i in 1..=15 {
			expected_lines.push(format!("E5_{:02}", i));
		}


		for i in 6..=9 {
			expected_lines.push(format!("LINE_{:02}", i));
		}


		expected_lines.push("MEGA_CONTRACTION_10_TO_20".to_string());


		for i in 21..=24 {
			expected_lines.push(format!("LINE_{:02}", i));
		}


		expected_lines.push("LINE_25".to_string());
		for i in 1..=8 {
			expected_lines.push(format!("I25_{}", i));
		}


		for i in 26..=29 {
			expected_lines.push(format!("LINE_{:02}", i));
		}


		for i in 1..=20 {
			expected_lines.push(format!("M30_{:02}", i));
		}


		for i in 36..=39 {
			expected_lines.push(format!("LINE_{:02}", i));
		}


		expected_lines.push("BIG_CONTRACT_A".to_string());
		expected_lines.push("BIG_CONTRACT_B".to_string());


		expected_lines.push("LINE_50".to_string());
		for i in 1..=5 {
			expected_lines.push(format!("FINAL_{}", i));
		}

		let expected = expected_lines.join("\n") + "\n";

		assert_eq!(
			actual, expected,
			"MASSIVE FILE: All operations must use original line positions!\nActual length: {}, Expected length: {}",
			actual.lines().count(), expected.lines().count()
		);
	}

	#[tokio::test]
	async fn test_batch_edit_pathological_case_all_expansions() {


		let temp_file = create_test_file("A\nB\nC\nD\nE\nF\nG\nH\nI\nJ\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let operations = json!([
			{
				"operation": "replace",
				"line_range": [1, 1],
				"content": "A1\nA2\nA3\nA4\nA5\nA6\nA7"
			},
			{
				"operation": "replace",
				"line_range": [3, 3],
				"content": "C1\nC2\nC3\nC4\nC5"
			},
			{
				"operation": "replace",
				"line_range": [5, 5],
				"content": "E1\nE2\nE3\nE4\nE5\nE6\nE7\nE8\nE9"
			},
			{
				"operation": "replace",
				"line_range": [7, 7],
				"content": "G01\nG02\nG03\nG04\nG05\nG06\nG07\nG08\nG09\nG10\nG11\nG12"
			},
			{
				"operation": "replace",
				"line_range": [9, 9],
				"content": "I1\nI2\nI3\nI4\nI5\nI6"
			}
		]);

		let call = create_batch_edit_call(&path, operations).await;
		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();


		let expected = "A1\nA2\nA3\nA4\nA5\nA6\nA7\nB\nC1\nC2\nC3\nC4\nC5\nD\nE1\nE2\nE3\nE4\nE5\nE6\nE7\nE8\nE9\nF\nG01\nG02\nG03\nG04\nG05\nG06\nG07\nG08\nG09\nG10\nG11\nG12\nH\nI1\nI2\nI3\nI4\nI5\nI6\nJ\n";

		assert_eq!(
			actual, expected,
			"PATHOLOGICAL: All expansions must preserve original positions!\nActual:\n{}\nExpected:\n{}",
			actual, expected
		);
	}





	#[tokio::test]
	async fn test_batch_edit_insert_after_n_and_replace_n_no_conflict() {
		let temp_file = create_test_file("AAA\nBBB\nCCC\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "insert", "line_range": 2, "content": "INSERTED"},
				{"operation": "replace", "line_range": [2, 2], "content": "REPLACED"}
			]),
		)
		.await;

		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();



		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "AAA\nREPLACED\nINSERTED\nCCC\n");
	}



	#[tokio::test]
	async fn test_batch_edit_insert_after_n_replace_range_including_n() {
		let temp_file = create_test_file("A\nB\nC\nD\nE\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "insert", "line_range": 3, "content": "NEW"},
				{"operation": "replace", "line_range": [2, 4], "content": "X\nY\nZ"}
			]),
		)
		.await;

		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();




		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "A\nX\nY\nNEW\nZ\nE\n");
	}



	#[tokio::test]
	async fn test_batch_edit_insert_at_zero_replace_line_one() {
		let temp_file = create_test_file("FIRST\nSECOND\nTHIRD\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "insert", "line_range": 0, "content": "HEADER"},
				{"operation": "replace", "line_range": [1, 1], "content": "REPLACED_FIRST"}
			]),
		)
		.await;

		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();




		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "HEADER\nREPLACED_FIRST\nSECOND\nTHIRD\n");
	}


	#[tokio::test]
	async fn test_batch_edit_two_replaces_overlapping_conflict() {
		let temp_file = create_test_file("A\nB\nC\nD\nE\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [1, 3], "content": "X"},
				{"operation": "replace", "line_range": [3, 5], "content": "Y"}
			]),
		)
		.await;

		let err = crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap_err();
		let result_str = err.to_string();
		assert!(
			result_str.contains("overlapping ranges"),
			"overlapping replaces [1,3] and [3,5] should conflict: {}",
			result_str
		);


		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "A\nB\nC\nD\nE\n");
	}


	#[tokio::test]
	async fn test_batch_edit_insert_replace_same_line_multiline_content() {
		let temp_file = create_test_file("A\nB\nC\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "insert", "line_range": 2, "content": "I1\nI2\nI3"},
				{"operation": "replace", "line_range": [2, 2], "content": "R1\nR2"}
			]),
		)
		.await;

		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();




		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "A\nR1\nR2\nI1\nI2\nI3\nC\n");
	}


	#[tokio::test]
	async fn test_batch_edit_multiple_inserts_with_spanning_replace() {
		let temp_file = create_test_file("A\nB\nC\nD\nE\nF\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "insert", "line_range": 1, "content": "AFTER_A"},
				{"operation": "replace", "line_range": [3, 4], "content": "REPLACED_CD"},
				{"operation": "insert", "line_range": 5, "content": "AFTER_E"}
			]),
		)
		.await;

		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();





		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "A\nAFTER_A\nB\nREPLACED_CD\nE\nAFTER_E\nF\n");
	}


	#[tokio::test]
	async fn test_batch_edit_two_replaces_adjacent_no_conflict() {
		let temp_file = create_test_file("A\nB\nC\nD\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [1, 2], "content": "X\nY"},
				{"operation": "replace", "line_range": [3, 4], "content": "W\nZ"}
			]),
		)
		.await;

		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "X\nY\nW\nZ\n");
	}


	#[tokio::test]
	async fn test_batch_edit_insert_after_last_replace_last() {
		let temp_file = create_test_file("A\nB\nC\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "insert", "line_range": 3, "content": "FOOTER"},
				{"operation": "replace", "line_range": [3, 3], "content": "REPLACED_C"}
			]),
		)
		.await;

		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();



		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "A\nB\nREPLACED_C\nFOOTER\n");
	}


	#[tokio::test]
	async fn test_batch_edit_delete_replace_with_insert_same_pos() {
		let temp_file = create_test_file("A\nB\nC\nD\nE\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [2, 3], "content": ""},
				{"operation": "insert", "line_range": 2, "content": "NEW"}
			]),
		)
		.await;

		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();




		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "A\nD\nNEW\nE\n");
	}


	#[tokio::test]
	async fn test_batch_edit_interleaved_inserts_and_replaces() {
		let temp_file = create_test_file("L1\nL2\nL3\nL4\nL5\nL6\n").await;
		let path = temp_file.path().to_string_lossy().to_string();
		let call = create_batch_edit_call(
			&path,
			json!([
				{"operation": "replace", "line_range": [1, 1], "content": "R1"},
				{"operation": "insert", "line_range": 1, "content": "I1"},
				{"operation": "replace", "line_range": [3, 3], "content": "R3"},
				{"operation": "insert", "line_range": 3, "content": "I3"},
				{"operation": "replace", "line_range": [5, 5], "content": "R5"},
				{"operation": "insert", "line_range": 5, "content": "I5"}
			]),
		)
		.await;

		crate::mcp::fs::core::execute_batch_edit(&call)
			.await
			.unwrap();








		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "R1\nI1\nL2\nR3\nI3\nL4\nR5\nI5\nL6\n");
	}



	#[tokio::test]
	async fn test_str_replace_fuzzy_whitespace_match() {

		let temp_file = create_test_file("fn hello() {\n    let x = 1;\n}").await;


		crate::mcp::fs::text_editing::str_replace_spec(
			temp_file.path(),
			"fn hello() {\n\tlet x = 1;\n}",
			"fn hello() {\n    let x = 2;\n}",
		)
		.await
		.unwrap();

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "fn hello() {\n    let x = 2;\n}");
	}

	#[tokio::test]
	async fn test_str_replace_fuzzy_indentation_adjustment() {

		let temp_file = create_test_file("class Foo {\n    def bar():\n        pass\n}").await;


		crate::mcp::fs::text_editing::str_replace_spec(
			temp_file.path(),
			"def bar():\n    pass",
			"def baz():\n    return 42",
		)
		.await
		.unwrap();

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "class Foo {\n    def baz():\n        return 42\n}");
	}

	#[tokio::test]
	async fn test_str_replace_error_shows_closest_matches() {
		let temp_file =
			create_test_file("fn hello_world() {}\nfn hello_earth() {}\nfn goodbye() {}").await;

		let err = crate::mcp::fs::text_editing::str_replace_spec(
			temp_file.path(),
			"fn hello_word() {}",
			"fn replaced() {}",
		)
		.await
		.unwrap_err();


		let content = err.to_string();
		assert!(
			content.contains("Closest matches"),
			"Should show closest matches in error: {}",
			content
		);
	}

	#[tokio::test]
	async fn test_str_replace_multi_level_undo() {
		let temp_file = create_test_file("version 1").await;


		for (old, new) in [
			("version 1", "version 2"),
			("version 2", "version 3"),
			("version 3", "version 4"),
		] {
			crate::mcp::fs::text_editing::str_replace_spec(temp_file.path(), old, new)
				.await
				.unwrap();
		}

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "version 4");


		for expected in ["version 3", "version 2", "version 1"] {
			crate::mcp::fs::core::undo_edit(temp_file.path())
				.await
				.unwrap();
			let actual = fs::read_to_string(temp_file.path()).await.unwrap();
			assert_eq!(actual, expected);
		}


		crate::mcp::fs::core::undo_edit(temp_file.path())
			.await
			.unwrap_err();
	}





	#[tokio::test]
	async fn test_hash_stability_after_edit() {

		let before = vec!["alpha", "beta", "gamma"];
		let hashes_before = crate::utils::line_hash::compute_line_hashes(&before);

		let after = vec!["alpha", "MODIFIED", "gamma"];
		let hashes_after = crate::utils::line_hash::compute_line_hashes(&after);


		assert_eq!(hashes_before[0], hashes_after[0], "alpha hash changed");
		assert_eq!(hashes_before[2], hashes_after[2], "gamma hash changed");

		assert_ne!(hashes_before[1], hashes_after[1], "beta hash should change");
	}

	#[tokio::test]
	async fn test_batch_edit_with_hash_line_range() {

		let content = "alpha\nbeta\ngamma\n";
		let temp_file = create_test_file(content).await;
		let path = temp_file.path().to_string_lossy().to_string();


		let lines: Vec<&str> = content.lines().collect();
		let hashes = crate::utils::line_hash::compute_line_hashes(&lines);


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "replace",
					"line_range": [&hashes[1], &hashes[1]],
					"content": "BETA_REPLACED"
				}]
			}),
		};

		execute_batch_edit(&call).await.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "alpha\nBETA_REPLACED\ngamma\n");
	}

	#[tokio::test]
	async fn test_batch_edit_hash_insert() {

		let content = "first\nsecond\nthird\n";
		let temp_file = create_test_file(content).await;
		let path = temp_file.path().to_string_lossy().to_string();

		let lines: Vec<&str> = content.lines().collect();
		let hashes = crate::utils::line_hash::compute_line_hashes(&lines);


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "insert",
					"line_range": &hashes[0],
					"content": "INSERTED"
				}]
			}),
		};

		execute_batch_edit(&call).await.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "first\nINSERTED\nsecond\nthird\n");
	}

	#[tokio::test]
	async fn test_batch_edit_hash_multi_line_replace() {

		let content = "a\nb\nc\nd\ne\n";
		let temp_file = create_test_file(content).await;
		let path = temp_file.path().to_string_lossy().to_string();

		let lines: Vec<&str> = content.lines().collect();
		let hashes = crate::utils::line_hash::compute_line_hashes(&lines);


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "replace",
					"line_range": [&hashes[1], &hashes[3]],
					"content": "REPLACED"
				}]
			}),
		};

		execute_batch_edit(&call).await.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "a\nREPLACED\ne\n");
	}

	#[tokio::test]
	async fn test_batch_edit_invalid_hash() {
		let content = "hello\nworld\n";
		let temp_file = create_test_file(content).await;
		let path = temp_file.path().to_string_lossy().to_string();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "replace",
					"line_range": ["zzzz", "zzzz"],
					"content": "nope"
				}]
			}),
		};

		let result = execute_batch_edit(&call).await;
		assert!(result.is_err());
		let err = result.unwrap_err().to_string();
		assert!(
			err.contains("not found"),
			"Error should mention hash not found: {}",
			err
		);
	}

	#[tokio::test]
	async fn test_hash_round_trip() {

		let lines = vec!["fn main() {", "    println!(\"hi\");", "}"];
		let hashes = crate::utils::line_hash::compute_line_hashes(&lines);

		for (i, hash) in hashes.iter().enumerate() {
			let resolved = crate::utils::line_hash::resolve_hash_to_line(hash, &lines).unwrap();
			assert_eq!(
				resolved,
				i + 1,
				"Hash {} should resolve to line {}",
				hash,
				i + 1
			);
		}
	}

	#[tokio::test]
	async fn test_batch_edit_swapped_hash_range_error() {

		let content = "line1\nline2\nline3\n";
		let temp_file = create_test_file(content).await;
		let path = temp_file.path().to_string_lossy().to_string();

		let lines: Vec<&str> = content.lines().collect();
		let hashes = crate::utils::line_hash::compute_line_hashes(&lines);


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "replace",
					"line_range": [&hashes[2], &hashes[0]],
					"content": "nope"
				}]
			}),
		};

		let err = execute_batch_edit(&call).await.unwrap_err().to_string();

		assert!(
			err.contains("reversed"),
			"Error should say range is reversed: {}",
			err
		);
		assert!(
			err.contains("Did you mean"),
			"Error should suggest correct order: {}",
			err
		);

		assert!(
			err.contains(&hashes[0]),
			"Error should contain the correct start hash: {}",
			err
		);
		assert!(
			err.contains(&hashes[2]),
			"Error should contain the correct end hash: {}",
			err
		);

		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, content);
	}

	#[tokio::test]
	async fn test_batch_edit_duplicate_content_lines_unique_hashes() {


		let content = "}\n}\n}\nfn foo() {\n";
		let temp_file = create_test_file(content).await;
		let path = temp_file.path().to_string_lossy().to_string();

		let lines: Vec<&str> = content.lines().collect();
		let hashes = crate::utils::line_hash::compute_line_hashes(&lines);


		let unique: std::collections::HashSet<&String> = hashes.iter().collect();
		assert_eq!(unique.len(), 4, "duplicate lines must get unique hashes");


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "batch_edit".to_string(),
			parameters: json!({
				"path": path,
				"operations": [{
					"operation": "replace",
					"line_range": [&hashes[1], &hashes[1]],
					"content": "// replaced"
				}]
			}),
		};

		execute_batch_edit(&call).await.unwrap();
		let actual = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(actual, "}\n// replaced\n}\nfn foo() {\n");
	}



	#[tokio::test]
	async fn test_view_file_content_search_basic() {
		let temp_file = create_test_file("alpha\nbeta\ngamma\ndelta\nepsilon\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({ "paths": path, "content": "gamma" }),
		};
		let output = execute_view(&call).await.unwrap();


		assert!(
			output.contains("3:gamma"),
			"expected '3:gamma', got: {output}"
		);

		assert!(
			!output.contains("gamma:"),
			"must not have rg colon format: {output}"
		);
		assert!(
			!output.contains("-gamma"),
			"must not have rg dash format: {output}"
		);
	}

	#[tokio::test]
	async fn test_view_file_content_search_no_match_returns_empty() {
		let temp_file = create_test_file("alpha\nbeta\ngamma\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({ "paths": path, "content": "zzznomatch" }),
		};
		let output = execute_view(&call).await.unwrap();
		assert!(
			output.is_empty(),
			"no match should return empty, got: {output}"
		);
	}

	#[tokio::test]
	async fn test_view_file_content_search_with_context() {
		let temp_file =
			create_test_file("alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({ "paths": path, "content": "gamma", "context": 1 }),
		};
		let output = execute_view(&call).await.unwrap();


		assert!(output.contains("2:beta"), "context before: {output}");
		assert!(output.contains("3:gamma"), "match line: {output}");
		assert!(output.contains("4:delta"), "context after: {output}");

		assert!(
			!output.contains("-beta"),
			"no rg dash prefix on context: {output}"
		);
		assert!(
			!output.contains("-delta"),
			"no rg dash prefix on context: {output}"
		);
	}

	#[tokio::test]
	async fn test_view_file_content_search_multiple_matches_separated() {
		let temp_file =
			create_test_file("alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\n").await;
		let path = temp_file.path().to_string_lossy().to_string();


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({ "paths": path, "content": "eta" }),
		};
		let output = execute_view(&call).await.unwrap();

		assert!(output.contains("7:eta"), "first match: {output}");
		assert!(output.contains("8:theta"), "second match: {output}");
	}

	#[tokio::test]
	async fn test_view_file_content_search_context_blocks_separated_by_dashes() {

		let temp_file = create_test_file("a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n").await;
		let path = temp_file.path().to_string_lossy().to_string();


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({ "paths": path, "content": "a" }),
		};
		let output = execute_view(&call).await.unwrap();

		assert!(output.contains("1:a"), "match: {output}");
		assert!(
			!output.contains("\n--\n"),
			"no separator for single block: {output}"
		);
	}

	#[tokio::test]
	async fn test_view_file_content_search_context_same_format_as_view_lines() {

		let temp_file = create_test_file("alpha\nbeta\ngamma\ndelta\nepsilon\n").await;
		let path = temp_file.path().to_string_lossy().to_string();


		let search_call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({ "paths": path, "content": "gamma" }),
		};
		let search_output = execute_view(&search_call).await.unwrap();


		let lines_call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({ "paths": path, "lines": [3, 3] }),
		};
		let lines_output = execute_view(&lines_call).await.unwrap();



		let search_line = search_output
			.lines()
			.find(|l| l.contains("gamma"))
			.unwrap_or("");
		let lines_line = lines_output
			.lines()
			.find(|l| l.contains("gamma"))
			.unwrap_or("");

		assert_eq!(
			search_line, lines_line,
			"content search and lines view must produce identical line format"
		);
	}

	#[tokio::test]
	async fn test_view_single_file_with_line_range() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let file_path = temp_dir.path().join("test.txt");
		fs::write(&file_path, "line 1\nline 2\nline 3\nline 4\nline 5\n")
			.await
			.unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": [file_path.to_string_lossy()],
				"lines": [2, 4]
			}),
		};

		let content = execute_view(&call).await.unwrap();
		assert!(
			content.contains("2:line 2"),
			"Should show line 2: {content}"
		);
		assert!(
			content.contains("3:line 3"),
			"Should show line 3: {content}"
		);
		assert!(
			content.contains("4:line 4"),
			"Should show line 4: {content}"
		);
	}

	#[tokio::test]
	async fn test_view_multi_file_with_per_file_ranges() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let file_a = temp_dir.path().join("a.txt");
		let file_b = temp_dir.path().join("b.txt");
		fs::write(&file_a, "a1\na2\na3\na4\na5\n").await.unwrap();
		fs::write(&file_b, "b1\nb2\nb3\nb4\nb5\n").await.unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": [file_a.to_string_lossy(), file_b.to_string_lossy()],
				"lines": [[2, 3], [4, 5]]
			}),
		};

		let content = execute_view(&call).await.unwrap();
		assert!(
			content.contains("2:a2") && content.contains("3:a3"),
			"File A should show lines 2-3: {content}"
		);
		assert!(
			content.contains("4:b4") && content.contains("5:b5"),
			"File B should show lines 4-5: {content}"
		);
	}

	#[tokio::test]
	async fn test_view_multi_file_with_single_range_applies_to_all() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let file_a = temp_dir.path().join("a.txt");
		let file_b = temp_dir.path().join("b.txt");
		fs::write(&file_a, "a1\na2\na3\na4\na5\n").await.unwrap();
		fs::write(&file_b, "b1\nb2\nb3\nb4\nb5\n").await.unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": [file_a.to_string_lossy(), file_b.to_string_lossy()],
				"lines": [1, 2]
			}),
		};

		let content = execute_view(&call).await.unwrap();
		assert!(
			content.contains("1:a1") && content.contains("2:a2"),
			"File A should show lines 1-2: {content}"
		);
		assert!(
			content.contains("1:b1") && content.contains("2:b2"),
			"File B should show lines 1-2: {content}"
		);
	}

	#[tokio::test]
	async fn test_view_multi_file_fewer_ranges_than_paths() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let file_a = temp_dir.path().join("a.txt");
		let file_b = temp_dir.path().join("b.txt");
		fs::write(&file_a, "a1\na2\na3\na4\na5\n").await.unwrap();
		fs::write(&file_b, "b1\nb2\nb3\nb4\nb5\n").await.unwrap();


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": [file_a.to_string_lossy(), file_b.to_string_lossy()],
				"lines": [[2, 3]]
			}),
		};

		let content = execute_view(&call).await.unwrap();
		assert!(
			content.contains("2:a2") && content.contains("3:a3"),
			"File A should show lines 2-3: {content}"
		);
		assert!(
			content.contains("1:b1") && content.contains("5:b5"),
			"File B should show full content: {content}"
		);
	}

	#[tokio::test]
	async fn test_view_multi_file_more_ranges_than_paths_errors() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let file_a = temp_dir.path().join("a.txt");
		let file_b = temp_dir.path().join("b.txt");
		fs::write(&file_a, "a1\na2\na3\n").await.unwrap();
		fs::write(&file_b, "b1\nb2\nb3\n").await.unwrap();


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": [file_a.to_string_lossy(), file_b.to_string_lossy()],
				"lines": [[1, 2], [3, 3], [1, 1]]
			}),
		};

		let err = execute_view(&call).await.unwrap_err();
		let msg = err.to_string();
		assert!(
			msg.contains("ranges") || msg.contains("paths"),
			"Should error about range/path count mismatch: {msg}"
		);
	}



	#[tokio::test]
	async fn test_view_lines_triple_wrapped_errors_with_hint() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let file_a = temp_dir.path().join("a.txt");
		fs::write(&file_a, "a1\na2\na3\na4\na5\n").await.unwrap();


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": [file_a.to_string_lossy()],
				"lines": [[[1, 3]]]
			}),
		};

		let err = execute_view(&call).await.unwrap_err();
		let msg = err.to_string();
		assert!(
			msg.contains("over-wrapped") && msg.contains("2 levels"),
			"Should hint about over-wrapping with 2-level max: {msg}"
		);
	}

	#[tokio::test]
	async fn test_view_lines_nested_non_array_element_errors_with_hint() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let file_a = temp_dir.path().join("a.txt");
		fs::write(&file_a, "a1\na2\na3\n").await.unwrap();


		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": [file_a.to_string_lossy()],
				"lines": [[1, 2], 3]
			}),
		};

		let err = execute_view(&call).await.unwrap_err();
		let msg = err.to_string();
		assert!(
			msg.contains("[start, end]") && msg.contains("Max 2 levels"),
			"Should hint about valid shapes and 2-level max: {msg}"
		);
	}

	#[tokio::test]
	async fn test_view_lines_flat_range_works() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let file_a = temp_dir.path().join("a.txt");
		fs::write(&file_a, "a1\na2\na3\na4\na5\n").await.unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": [file_a.to_string_lossy()],
				"lines": [2, 4]
			}),
		};

		let content = execute_view(&call).await.unwrap();
		assert!(
			content.contains("2:a2") && content.contains("4:a4"),
			"Flat range should show lines 2-4: {content}"
		);
	}

	#[tokio::test]
	async fn test_view_lines_nested_ranges_single_path_works() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let file_a = temp_dir.path().join("a.txt");
		fs::write(&file_a, "a1\na2\na3\na4\na5\na6\n")
			.await
			.unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": [file_a.to_string_lossy()],
				"lines": [[1, 2], [5, 6]]
			}),
		};

		let content = execute_view(&call).await.unwrap();
		assert!(
			content.contains("1:a1")
				&& content.contains("2:a2")
				&& content.contains("5:a5")
				&& content.contains("6:a6"),
			"Nested ranges on single path should show lines 1-2 and 5-6: {content}"
		);
	}

	#[cfg(unix)]
	#[tokio::test]
	async fn test_atomic_write_preserves_executable_bit() {
		use crate::mcp::fs::text_editing::atomic_write;
		use std::os::unix::fs::PermissionsExt;

		let temp_dir = tempfile::TempDir::new().unwrap();
		let file_path = temp_dir.path().join("script.sh");
		fs::write(&file_path, "#!/bin/sh\necho old\n")
			.await
			.unwrap();
		fs::set_permissions(&file_path, std::fs::Permissions::from_mode(0o755))
			.await
			.unwrap();

		atomic_write(&file_path, "#!/bin/sh\necho new\n")
			.await
			.unwrap();

		let mode = fs::metadata(&file_path).await.unwrap().permissions().mode() & 0o777;
		assert_eq!(
			mode, 0o755,
			"atomic_write must preserve original mode (got {mode:o})"
		);
	}
	#[tokio::test]
	async fn test_text_editor_delete_removes_file_and_supports_undo() {
		let temp_file = create_test_file("hello world\n").await;
		let path = temp_file.path().to_string_lossy().to_string();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "text_editor".to_string(),
			parameters: json!({
				"command": "delete",
				"path": path,
			}),
		};
		let result = crate::mcp::fs::core::execute_text_editor(&call)
			.await
			.unwrap();
		assert!(result.contains("Successfully deleted"), "got: {result}");
		assert!(!temp_file.path().exists(), "file should be gone");

		let undo_call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "text_editor".to_string(),
			parameters: json!({
				"command": "undo_edit",
				"path": path,
			}),
		};
		crate::mcp::fs::core::execute_text_editor(&undo_call)
			.await
			.unwrap();
		let restored = fs::read_to_string(temp_file.path()).await.unwrap();
		assert_eq!(restored, "hello world\n");
	}

	#[tokio::test]
	async fn test_text_editor_delete_missing_file_errors() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let path = temp_dir
			.path()
			.join("nope.txt")
			.to_string_lossy()
			.to_string();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "text_editor".to_string(),
			parameters: json!({
				"command": "delete",
				"path": path,
			}),
		};
		let err = crate::mcp::fs::core::execute_text_editor(&call)
			.await
			.unwrap_err();
		assert!(err.to_string().contains("does not exist"), "got: {err}");
	}

	#[tokio::test]
	async fn test_text_editor_delete_directory_errors() {
		let temp_dir = tempfile::TempDir::new().unwrap();
		let path = temp_dir.path().to_string_lossy().to_string();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: std::env::current_dir().unwrap_or_default(),
			tool_name: "text_editor".to_string(),
			parameters: json!({
				"command": "delete",
				"path": path,
			}),
		};
		let err = crate::mcp::fs::core::execute_text_editor(&call)
			.await
			.unwrap_err();
		assert!(err.to_string().contains("directory"), "got: {err}");
	}



	#[tokio::test]
	async fn test_view_regex_alternation_in_directory() {
		use std::fs as stdfs;
		use tempfile::TempDir;

		let dir = TempDir::new().unwrap();
		stdfs::write(dir.path().join("a.txt"), "TODO: fix\nok\nFIXME: bug\n").unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: dir.path().to_path_buf(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": [dir.path().to_str().unwrap()],
				"content": "TODO|FIXME",
				"regex": true,
			}),
		};
		let out = execute_view(&call).await.unwrap();
		assert!(out.contains("TODO: fix"), "got: {out}");
		assert!(out.contains("FIXME: bug"), "got: {out}");
	}

	#[tokio::test]
	async fn test_view_regex_case_insensitive() {
		use std::fs as stdfs;
		use tempfile::TempDir;

		let dir = TempDir::new().unwrap();
		stdfs::write(dir.path().join("log.txt"), "ok\nERROR: boom\nError again\n").unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: dir.path().to_path_buf(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": [dir.path().to_str().unwrap()],
				"content": "(?i)error",
				"regex": true,
			}),
		};
		let out = execute_view(&call).await.unwrap();
		assert!(out.contains("ERROR: boom"), "got: {out}");
		assert!(out.contains("Error again"), "got: {out}");
	}

	#[tokio::test]
	async fn test_view_regex_invalid_pattern_errors() {
		use tempfile::TempDir;

		let dir = TempDir::new().unwrap();
		std::fs::write(dir.path().join("a.txt"), "anything\n").unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: dir.path().to_path_buf(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": [dir.path().to_str().unwrap()],
				"content": "[unclosed",
				"regex": true,
			}),
		};
		let err = execute_view(&call).await.unwrap_err();
		assert!(
			err.to_string().to_lowercase().contains("regex"),
			"got: {err}"
		);
	}

	#[tokio::test]
	async fn test_view_literal_treats_regex_chars_as_text() {

		use std::fs as stdfs;
		use tempfile::TempDir;

		let dir = TempDir::new().unwrap();
		stdfs::write(dir.path().join("a.rs"), "fn main()\nlet x = 1;\n").unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: dir.path().to_path_buf(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": [dir.path().to_str().unwrap()],
				"content": "main()",
			}),
		};
		let out = execute_view(&call).await.unwrap();
		assert!(out.contains("fn main()"), "got: {out}");
	}

	#[tokio::test]
	async fn test_view_finds_match_in_latin1_file() {



		use std::fs as stdfs;
		use tempfile::TempDir;

		let dir = TempDir::new().unwrap();
		let mut bytes = b"caf".to_vec();
		bytes.push(0xE9);
		bytes.extend_from_slice(b" MARKER here\nplain ascii\n");
		stdfs::write(dir.path().join("non_utf8.txt"), &bytes).unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: dir.path().to_path_buf(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": [dir.path().to_str().unwrap()],
				"content": "MARKER",
			}),
		};
		let out = execute_view(&call).await.unwrap();
		assert!(out.contains("MARKER here"), "got: {out}");
	}

	#[tokio::test]
	async fn test_view_single_file_regex_search() {
		use std::fs as stdfs;
		use tempfile::TempDir;

		let dir = TempDir::new().unwrap();
		let file = dir.path().join("code.rs");
		stdfs::write(&file, "fn alpha() {}\nfn beta() {}\nstruct Gamma;\n").unwrap();

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: dir.path().to_path_buf(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": [file.to_str().unwrap()],
				"content": r"^fn \w+",
				"regex": true,
			}),
		};
		let out = execute_view(&call).await.unwrap();
		assert!(out.contains("fn alpha"), "got: {out}");
		assert!(out.contains("fn beta"), "got: {out}");
		assert!(!out.contains("Gamma"), "regex anchored to fn — got: {out}");
	}

	#[tokio::test]
	async fn test_view_parallel_scan_preserves_order() {

		use std::fs as stdfs;
		use tempfile::TempDir;

		let dir = TempDir::new().unwrap();
		for i in 0..40 {
			stdfs::write(
				dir.path().join(format!("file_{i:03}.txt")),
				format!("HIT line in file {i}\n"),
			)
			.unwrap();
		}

		let call = McpToolCall {
			tool_id: "test".to_string(),
			workdir: dir.path().to_path_buf(),
			tool_name: "view".to_string(),
			parameters: json!({
				"paths": [dir.path().to_str().unwrap()],
				"content": "HIT",
			}),
		};
		let out = execute_view(&call).await.unwrap();


		let p000 = out.find("file_000.txt").expect("file_000 missing");
		let p039 = out.find("file_039.txt").expect("file_039 missing");
		assert!(p000 < p039, "ordering not preserved");

		assert_eq!(out.matches("HIT line").count(), 40);
	}
}
