import io
import json
import subprocess
import zipfile

import pytest
import snapshot_review as sr

PNG = sr.PNG_MAGIC + b"rest of the image"


def report(outcome="failure", suite="Agent GUI", version=1):
    return json.dumps({"version": version, "suite": suite, "outcome": outcome}).encode()


def failing_files(n):
    files = {"report.json": report()}
    for i in range(n):
        files[f"snap_{i}.png"] = PNG + b"ref"
        files[f"snap_{i}.new.png"] = PNG + b"new"
        files[f"snap_{i}.diff.png"] = PNG + b"diff"
    return files


def suite(files, slug="agent-gui"):
    return sr.parse_artifact(
        slug, files, f"snapshot-review-{slug}", "https://example.invalid/artifact"
    )


def test_parse_failures_sorted_with_optional_images():
    files = failing_files(2)
    del files["snap_1.diff.png"]
    s = suite(files)
    assert s.title == "Agent GUI"
    assert [f.name for f in s.failures] == ["snap_0", "snap_1"]
    assert s.failures[0].diff == PNG + b"diff"
    assert s.failures[1].diff is None


def test_parse_ignores_images_when_tests_passed():
    files = failing_files(1)
    files["report.json"] = report("success")
    assert suite(files).failures == []


def test_parse_ignores_files_that_are_not_png():
    files = failing_files(1)
    files["snap_0.new.png"] = b"<script>"
    assert suite(files).failures == []


@pytest.mark.parametrize(
    "report_json",
    [
        None,
        b"not json",
        report(version=2),
        report(outcome="maybe"),
        json.dumps({"version": 1, "outcome": "success"}).encode(),
    ],
)
def test_parse_rejects_bad_reports(report_json):
    files = {} if report_json is None else {"report.json": report_json}
    with pytest.raises(sr.InvalidArtifact):
        suite(files)


def test_title_is_cleaned():
    files = {"report.json": report(suite="<img src=x> Agent | GUI")}
    assert suite(files).title == "img srcx Agent  GUI"


def test_zip_keeps_only_flat_files_with_safe_names():
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as zf:
        zf.writestr("report.json", "{}")
        zf.writestr("a.new.png", PNG)
        zf.writestr("../evil.png", PNG)
        zf.writestr("sub/b.png", PNG)
        zf.writestr(".hidden.png", PNG)
    assert sorted(sr.read_artifact_zip(buf.getvalue())) == ["a.new.png", "report.json"]


def test_comment_shows_at_most_five_failures_and_links_the_artifact():
    s = suite(failing_files(7))
    body = sr.render_comment(
        "owner/repo", "abc123", "https://example.invalid/run", "pr-3/9-1", [s]
    )
    assert body.startswith(sr.MARKER)
    assert "Visual tests failed in abc123" in body
    assert "7 failing snapshots" in body
    assert "<summary>Show 5 of 7</summary>" in body
    assert body.count("<details>") == 1
    assert "`snap_4`" in body and "`snap_5`" not in body
    assert (
        "2 more failures and all images are in the artifact [snapshot-review-agent-gui]("
        in body
    )
    assert (
        "https://raw.githubusercontent.com/owner/repo/ci-snapshot-review/pr-3/9-1/agent-gui/snap_0.diff.png"
        in body
    )


def test_comment_with_a_passing_and_a_failing_suite():
    passing = {"report.json": report("success", suite="Web GUI")}
    body = sr.render_comment(
        "o/r",
        "abc",
        "u",
        "pr-1/1-1",
        [suite(passing, "web-gui"), suite(failing_files(1))],
    )
    assert "Visual tests failed in abc" in body
    assert "✅ Web GUI: all visual tests pass" in body
    assert "1 failing snapshot\n" in body
    assert "All images are in the artifact [" in body
    assert body.index("Agent GUI") < body.index("Web GUI")


def test_comment_when_tests_fail_without_differing_snapshots():
    body = sr.render_comment("o/r", "abc", "u", "d", [suite({"report.json": report()})])
    assert "visual tests failed, but no snapshot differs" in body


def test_comment_when_everything_passes():
    body = sr.render_comment(
        "o/r", "abc", "u", "d", [suite({"report.json": report("success")})]
    )
    assert "Visual tests pass as of abc" in body


def test_published_files_are_the_shown_images():
    s = suite(failing_files(7))
    files = sr.published_files("pr-3/9-1", [s])
    assert len(files) == 15
    assert files["pr-3/9-1/agent-gui/snap_0.png"] == PNG + b"ref"


def test_base_tree_keeps_only_open_pulls():
    tree = {
        "README.md": "x",
        "pr-1/a.png": "b1",
        "pr-2/a.png": "b2",
        "pr-3/a.png": "b3",
        "stray.txt": "b4",
    }
    kept = sr.base_tree(tree, open_pulls={1, 3}, drop_pull=3)
    assert kept == {"README.md": sr.BRANCH_README, "pr-1/a.png": "b1"}


# ---- the branch rewrite against a local "origin" -----------------------------------------------


def git(cwd, *args):
    return subprocess.run(
        ["git", *args], cwd=cwd, check=True, capture_output=True, text=True
    ).stdout.strip()


@pytest.fixture
def clone(tmp_path, monkeypatch):
    origin = tmp_path / "origin.git"
    git(tmp_path, "init", "--quiet", "--bare", str(origin))
    work = tmp_path / "work"
    git(tmp_path, "init", "--quiet", str(work))
    git(work, "remote", "add", "origin", str(origin))
    monkeypatch.chdir(work)
    return origin


def branch_files(origin):
    out = git(origin, "ls-tree", "-r", "--name-only", sr.BRANCH)
    return sorted(out.splitlines())


def test_update_branch_creates_and_rewrites_a_single_commit(clone):
    sr.update_branch(
        lambda tree: {
            **sr.base_tree(tree, {1, 2}),
            "pr-1/r/a.png": b"1",
            "pr-2/r/a.png": b"2",
        },
        "first",
    )
    assert branch_files(clone) == ["README.md", "pr-1/r/a.png", "pr-2/r/a.png"]

    # PR 2 got closed, PR 1 has a new run
    sr.update_branch(
        lambda tree: {**sr.base_tree(tree, {1}, drop_pull=1), "pr-1/s/b.png": b"3"},
        "second",
    )
    assert branch_files(clone) == ["README.md", "pr-1/s/b.png"]
    assert git(clone, "rev-list", "--count", sr.BRANCH) == "1"
    assert git(clone, "log", "-1", "--format=%s", sr.BRANCH) == "second"


def test_update_branch_skips_a_push_without_changes(clone):
    sr.update_branch(lambda tree: sr.base_tree(tree, set()), "first")
    before = git(clone, "rev-parse", sr.BRANCH)
    sr.update_branch(lambda tree: sr.base_tree(tree, set()), "second")
    assert git(clone, "rev-parse", sr.BRANCH) == before


def test_update_branch_retries_after_a_concurrent_push(clone):
    calls = []

    def change(tree):
        calls.append(sorted(tree))
        if len(calls) == 1:
            # another run pushes after this one has read the branch
            sr.update_branch(
                lambda t: {**sr.base_tree(t, {1, 2}), "pr-2/r/a.png": b"2"},
                "concurrent",
            )
        return {**sr.base_tree(tree, {1, 2}), "pr-1/r/a.png": b"1"}

    sr.update_branch(lambda tree: sr.base_tree(tree, set()), "initial")
    sr.update_branch(change, "mine")
    assert len(calls) == 2
    assert branch_files(clone) == ["README.md", "pr-1/r/a.png", "pr-2/r/a.png"]
