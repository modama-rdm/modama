//! Python code for a Jupyter notebook that asks the local agent for the current session folder

/// The agent API version the snippet asks for; the foreground endpoint exists since 1.0.0
const API_VERSION: &str = "1.0.0";

/// Python code (standard library only) that sets `session_folder` to the foreground share's
/// mount path, as reported by the agent at `agent_url`, `raw_folder` to its `raw`
/// subfolder, and `project` and `session` to their names. It asks at run time, so a notebook rerun later writes into the session open
/// then, never into a stale one. It also defines `free_subfolder()`, which names the next
/// numbered subfolder of the raw folder; that function needs Python 3.9.
pub fn python_snippet(agent_url: &str) -> String {
    let url = format!("{}/api/shares/foreground/", agent_url.trim_end_matches('/'));
    format!(
        r#"# The session folder for new data (the foreground ingest),
# from the modama agent on this computer. Run this before you
# start saving.
import json
import pathlib
import urllib.request

url = "{url}"
headers = {{"X-API-Version": "{API_VERSION}"}}
request = urllib.request.Request(url, headers=headers)
with urllib.request.urlopen(request, timeout=10) as response:
    share = json.load(response)
if "mount_path" not in share:
    hint = "No foreground ingest: start one in the modama agent"
    raise RuntimeError(share.get("message", hint))
project, session = share["project"], share["session"]
session_folder = share["mount_path"]
raw_folder = pathlib.Path(session_folder) / "raw"


# A subfolder of the raw folder that doesn't exist yet, numbered
# after the highest one there: scan_0001, scan_0002, ...
# With full_path=False, only its name. Create it yourself.
def free_subfolder(prefix="scan_", digits=4, full_path=True):
    numbers = [0]
    for path in raw_folder.iterdir():
        rest = path.name.removeprefix(prefix)
        if path.name.startswith(prefix) and rest.isdecimal():
            numbers.append(int(rest))
    name = f"{{prefix}}{{max(numbers) + 1:0{{digits}}}}"
    return raw_folder / name if full_path else name
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
    fn the_snippet_defines_free_subfolder_with_its_defaults() {
        let snippet = python_snippet("http://localhost:3003");
        assert!(
            snippet.contains(r#"def free_subfolder(prefix="scan_", digits=4, full_path=True):"#)
        );
        assert!(snippet.contains(r#"f"{prefix}{max(numbers) + 1:0{digits}}""#));
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
