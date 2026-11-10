//! Linux notification-area icon: a StatusNotifierItem through ksni, on the helper's tokio
//! runtime (no GLib involved).

use ksni::TrayMethods as _;
use log::info;
use tokio::sync::mpsc;

use crate::tray::{TrayAction, TrayState};

const ICON_32: &[u8] = include_bytes!("../assets/tray-icon-32.rgba");
const ICON_64: &[u8] = include_bytes!("../assets/tray-icon-64.rgba");

/// The committed RGBA pixels in the ARGB32 (network byte order) the protocol wants
fn pixmap(size: i32, rgba: &[u8]) -> ksni::Icon {
    let data = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| [a, r, g, b])
        .collect();
    ksni::Icon {
        width: size,
        height: size,
        data,
    }
}

struct ModamaTray {
    state: TrayState,
    actions: mpsc::UnboundedSender<TrayAction>,
}

impl ModamaTray {
    fn request(&self, action: TrayAction) {
        // the receiver only goes away with the helper
        let _ = self.actions.send(action);
    }
}

impl ksni::Tray for ModamaTray {
    fn id(&self) -> String {
        "modama-session-helper".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::SystemServices
    }

    fn title(&self) -> String {
        "modama-agent".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![pixmap(32, ICON_32), pixmap(64, ICON_64)]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "modama-agent".into(),
            description: self.state.label(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.request(TrayAction::OpenModama);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: self.state.label(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Open modama-agent".into(),
                activate: Box::new(|tray: &mut Self| tray.request(TrayAction::OpenModama)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Open ingest share".into(),
                enabled: self.state.network_drive.is_some(),
                activate: Box::new(|tray: &mut Self| tray.request(TrayAction::OpenNetworkDrive)),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        // keep the item: the desktop may provide a host later (login, shell restart)
        info!("the notification area went away ({reason:?}); the icon shows again once it is back");
        true
    }
}

pub struct TrayItem {
    handle: ksni::Handle<ModamaTray>,
}

/// Publish the item; fails only without a session bus. A desktop that has no notification area
/// (yet) doesn't fail it: the item waits for a host.
pub async fn start(
    state: TrayState,
    actions: mpsc::UnboundedSender<TrayAction>,
) -> Result<TrayItem, String> {
    let handle = ModamaTray { state, actions }
        .assume_sni_available(true)
        .spawn()
        .await
        .map_err(|e| e.to_string())?;
    Ok(TrayItem { handle })
}

impl TrayItem {
    pub async fn update(&self, state: TrayState) {
        self.handle.update(move |tray| tray.state = state).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_are_converted_to_argb() {
        let icon = pixmap(1, &[1, 2, 3, 4]);
        assert_eq!(icon.data, vec![4, 1, 2, 3]);
        assert_eq!(ICON_32.len(), 32 * 32 * 4);
        assert_eq!(ICON_64.len(), 64 * 64 * 4);
    }
}
