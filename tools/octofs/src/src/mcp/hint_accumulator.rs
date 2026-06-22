




















use std::sync::Mutex;

static HINTS: Mutex<Vec<String>> = Mutex::new(Vec::new());


pub fn push_hint(hint: &str) {
	if let Ok(mut hints) = HINTS.lock() {
		hints.push(hint.to_string());
	}
}



pub fn drain_hints() -> Vec<String> {
	let Ok(mut hints) = HINTS.lock() else {
		return Vec::new();
	};
	let mut seen = std::collections::HashSet::new();
	hints.drain(..).filter(|h| seen.insert(h.clone())).collect()
}


pub fn has_hints() -> bool {
	HINTS.lock().is_ok_and(|h| !h.is_empty())
}
