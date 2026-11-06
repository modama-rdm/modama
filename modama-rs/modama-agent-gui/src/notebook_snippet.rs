//! Python code for a Jupyter notebook that asks the local agent for the current session folder

/// The agent API version the snippet asks for; the foreground endpoint exists since 1.0.0
const API_VERSION: &str = "1.0.0";

/// Python code (standard library only) that sets `session_folder` to the foreground share's
/// mount path, as reported by the agent at `agent_url`. It asks at run time, so a notebook
/// rerun later writes into the session open then, never into a stale one.
pub fn python_snippet(agent_url: &str) -> String {
    let url = format!("{}/api/shares/foreground/", agent_url.trim_end_matches('/'));
    format!(
        r#"# The session folder for new data (the foreground ingest), from
# the modama agent on this computer. Run this when you start
# saving: it changes whenever someone starts a new ingest.
import json
import urllib.request

url = "{url}"
headers = {{"X-API-Version": "{API_VERSION}"}}
request = urllib.request.Request(url, headers=headers)
with urllib.request.urlopen(request, timeout=10) as response:
    share = json.load(response)
if "mount_path" not in share:
    reason = share.get("message", "No foreground ingest.")
    raise RuntimeError(reason)
session_folder = share["mount_path"]
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_snippet_lines_fit_the_code_box() {
        let snippet = python_snippet("http://localhost:3003");
        let longest = snippet.lines().map(str::len).max().unwrap_or_default();
        assert!(longest <= 64, "{longest}");
    }

    #[test]
    fn the_snippet_asks_the_configured_agent() {
        let snippet = python_snippet("http://localhost:3003/");
        assert!(snippet.contains(r#""http://localhost:3003/api/shares/foreground/""#));
        assert!(snippet.contains(r#"{"X-API-Version": "1.0.0"}"#));
        // JetBrains Mono's ligatures would show these as single symbols
        for ligature in ["!=", "==", "->", "<=", ">="] {
            assert!(!snippet.contains(ligature), "{ligature}");
        }
    }
}
