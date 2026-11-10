//! End to end: the agents connected at the resource, as agent1's GUI lists them.
//!
//! test-env runs three agents at the resource: agent1 and agent2, enrolled, and an anonymous
//! one, which takes part with a token from an enrolled agent. Each is listed on its own row
//! with its identity and mount status, which only agent1 has a session helper for: what
//! smbdrop and the agents tell each other, and the agent the GUI. See `e2e_common` for how the
//! tests run.
#![cfg(test)]
#![cfg(feature = "e2e")]

mod e2e_common;

use std::time::Duration;

use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::Queryable as _;
use egui_phosphor::regular::{INFO, LOCK_SIMPLE};

use e2e_common::{AppHarness, gui, has_text, resource, settle, text, texts, wait_until};

const STATUSES: [&str; 5] = [
    "idle",
    "reconciling",
    "reconciled, with work left over",
    "no session helper",
    "status unknown",
];

/// One row of the list: `● name 🔒 … this agent`
#[derive(Debug)]
struct AgentRow {
    name: String,
    verified: bool,
    own: bool,
    name_rect: egui::Rect,
    icon_rect: egui::Rect,
}

fn labels(harness: &AppHarness) -> Vec<(String, egui::Rect)> {
    harness
        .query_all_by_role(Role::Label)
        .filter_map(|node| Some((text(&node)?, node.rect())))
        .collect()
}

/// The rows below the heading: each is an identity icon, with the name to its left on the
/// same line
fn agent_rows(harness: &AppHarness, heading: &str) -> Vec<AgentRow> {
    let labels = labels(harness);
    let Some(top) = labels
        .iter()
        .find(|(text, _)| text == heading)
        .map(|(_, rect)| rect.bottom())
    else {
        return Vec::new();
    };
    let same_line = |a: &egui::Rect, b: &egui::Rect| (a.center().y - b.center().y).abs() < 4.0;
    labels
        .iter()
        .filter(|(text, rect)| (text == LOCK_SIMPLE || text == INFO) && rect.top() > top)
        .filter_map(|(icon, icon_rect)| {
            let (name, name_rect) = labels
                .iter()
                .filter(|(_, rect)| same_line(rect, icon_rect) && rect.right() <= icon_rect.left())
                .max_by(|a, b| a.1.right().total_cmp(&b.1.right()))?;
            Some(AgentRow {
                name: name.clone(),
                verified: icon == LOCK_SIMPLE,
                own: labels
                    .iter()
                    .any(|(text, rect)| text == "this agent" && same_line(rect, icon_rect)),
                name_rect: *name_rect,
                icon_rect: *icon_rect,
            })
        })
        .collect()
}

/// The tooltip shown while the pointer is at `pos`, if one of `candidates` is in it
fn tooltip_at(harness: &mut AppHarness, pos: egui::Pos2, candidates: &[&str]) -> Option<String> {
    harness.hover_at(pos);
    settle(harness);
    let found = texts(harness)
        .into_iter()
        .find(|text| candidates.iter().any(|candidate| text.contains(candidate)));
    harness.hover_at(egui::pos2(1.0, 1.0));
    settle(harness);
    found
}

/// The mount status on the row's dot, which is left of the name and says it only on hover
fn status(harness: &mut AppHarness, row: &AgentRow) -> Option<String> {
    (2..40).step_by(2).find_map(|dx| {
        let pos = egui::pos2(row.name_rect.left() - dx as f32, row.name_rect.center().y);
        tooltip_at(harness, pos, &STATUSES)
    })
}

#[test]
fn each_agent_at_the_resource_on_its_own_row() {
    let heading = format!("Connected agents at {}", resource());
    let mut harness = gui();
    wait_until(
        &mut harness,
        "the three agents at the resource",
        Duration::from_secs(60),
        |h| has_text(h, &heading) && agent_rows(h, &heading).len() >= 3,
    );
    let rows = agent_rows(&harness, &heading);
    let row = |name: &str| {
        rows.iter()
            .find(|row| row.name == name)
            .unwrap_or_else(|| panic!("no row for {name}: {rows:?}"))
    };

    let agent1 = row("agent1");
    assert!(agent1.verified && agent1.own, "{agent1:?}");
    let agent2 = row("agent2");
    assert!(agent2.verified && !agent2.own, "{agent2:?}");
    let anonymous: Vec<&AgentRow> = rows.iter().filter(|row| !row.verified).collect();
    assert_eq!(anonymous.len(), 1, "one anonymous agent expected: {rows:?}");
    let anonymous = anonymous[0];
    assert!(!anonymous.own, "{anonymous:?}");

    // the anonymous agent's token comes from an enrolled agent
    let identity = tooltip_at(&mut harness, anonymous.icon_rect.center(), &["Token from "])
        .unwrap_or_else(|| panic!("no identity details for {}", anonymous.name));
    assert!(
        identity.contains("Token from agent1") || identity.contains("Token from agent2"),
        "unexpected identity details: {identity:?}"
    );

    // only agent1 has a session helper (session-1); it may still be busy with a pass
    let mut agent1_status = status(&mut harness, agent1);
    for _ in 0..20 {
        if agent1_status.as_deref() == Some("idle") {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
        agent1_status = status(&mut harness, agent1);
    }
    assert_eq!(agent1_status.as_deref(), Some("idle"));
    for other in [agent2, anonymous] {
        assert_eq!(
            status(&mut harness, other).as_deref(),
            Some("no session helper"),
            "{}",
            other.name
        );
    }
}
