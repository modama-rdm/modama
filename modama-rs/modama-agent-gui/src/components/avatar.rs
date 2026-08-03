use eframe::egui::{self, Color32, Sense, };

use crate::styles;

const IDENTITY_COLORS: [Color32; 8] = [
    Color32::from_rgb(0x25, 0x63, 0xeb),
    Color32::from_rgb(0x7c, 0x3a, 0xed),
    Color32::from_rgb(0x05, 0x96, 0x69),
    Color32::from_rgb(0xd9, 0x77, 0x06),
    Color32::from_rgb(0xdc, 0x26, 0x26),
    Color32::from_rgb(0x08, 0x91, 0xb2),
    Color32::from_rgb(0xbe, 0x18, 0x5d),
    Color32::from_rgb(0x65, 0xa3, 0x0d),
];

/// Render a deterministic user avatar at an explicitly selected size.
pub fn avatar(ui: &mut egui::Ui, username: &str, size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(size), Sense::hover());
    let color = IDENTITY_COLORS[username_hash(username) % IDENTITY_COLORS.len()];
    ui.painter().circle_filled(rect.center(), size / 2.0, color);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initials(username),
        styles::typography::button::font_id(),
        Color32::WHITE,
    );
    response.on_hover_text(username)
}

fn username_hash(username: &str) -> usize {
    username
        .chars()
        .fold(0_i32, |hash, character| {
            hash.wrapping_mul(31)
                .wrapping_add(i32::try_from(u32::from(character)).unwrap_or_default())
        })
        .unsigned_abs() as usize
}

fn initials(username: &str) -> String {
    username.chars().take(2).collect::<String>().to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_identity_is_stable() {
        assert_eq!(initials("alex"), "AL");
        assert_eq!(username_hash("alex"), username_hash("alex"));
        assert_ne!(username_hash("alex"), username_hash("sam"));
    }
}
