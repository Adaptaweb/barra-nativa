//! DPI/theme-aware icon and label cache; bounded shell asset delivery.

use anyhow::Result;
use muelle_core::frame::DockEdge;
use muelle_shell::{icons::IconImage, recycle_bin, tracker};

use crate::{assets::fallback_name, strings::text};

use super::{
    App, Asset, GROUP_LABEL_CHARS, Role, START_ID, WINDOW_TITLE_CHARS, preferences::explorer_id,
    shorten, window_of,
};

/// The parts of a widget cell a click acts on, as shares of its width: the
/// dock magnifies cells, so pixels would not hold.
fn shares(
    widget: crate::widgets::WidgetId,
    wanted: u32,
    zones: Vec<(f32, f32, crate::widgets::CellAction)>,
) -> Vec<(f32, f32, crate::widgets::CellAction)> {
    let hundredths = crate::widgets::dock_width(widget, crate::widgets::dock_size(widget));
    let width = (wanted as f32 * f32::from(hundredths) / 100.0).max(1.0);
    zones
        .into_iter()
        .map(|(from, to, action)| (from / width, to / width, action))
        .collect()
}

impl App {
    /// The action of the button under the pointer in the widget cell `index`
    /// (`id`), if the pointer is on one.
    pub(super) fn widget_action(&self, index: usize, id: &str) -> Option<crate::widgets::CellAction> {
        let stage = self.stage.as_ref()?;
        let state = muelle_core::magnify::magnify(
            &stage.layout,
            self.cursor,
            self.amount.value,
            &stage.magnifier,
        );
        let rect = state.rects.get(index)?;
        let share = (self.cursor - rect.x) / rect.width;
        self.assets
            .get(id)?
            .zones
            .iter()
            .find(|(from, to, _)| share >= *from && share < *to)
            .map(|(_, _, action)| *action)
    }

    /// `wanted` is the pixel side icons are drawn at: their magnified size.
    pub(super) fn prepare_assets(&mut self, wanted: u32, scale: f32, light: bool, edge: DockEdge) {
        if self.assets_for != (scale, light, wanted, edge) {
            self.assets.clear();
            self.remove_label = None;
            self.placeholder = None;
            self.assets_for = (scale, light, wanted, edge);
        }
        if self.remove_label.is_none() {
            self.remove_label = self.gfx.label_brush(text().remove, scale, light, edge).ok();
        }
        if self.placeholder.is_none() {
            self.placeholder = self.gfx.placeholder_brush(wanted, light).ok();
        }
        for entry in &self.entries {
            let Some(id) = entry.id.as_ref() else {
                continue;
            };
            if entry.role == Role::Group {
                if self
                    .assets
                    .get(id)
                    .is_some_and(|asset| asset.drawn_for == entry.members)
                {
                    continue;
                }
                // Every app is asked for, not only the four on the tile: the
                // unfolded group shows them all.
                for member in &entry.members {
                    let _ = self.icons.get(member, "");
                }
                let images: Vec<Option<&IconImage>> = entry
                    .members
                    .iter()
                    .take(4)
                    .map(|member| self.icons.peek(member, "").map(|visual| &visual.icon))
                    .collect();
                let icon = self
                    .gfx
                    .group_brush(wanted, &images, entry.members.len(), light)
                    .inspect_err(|error| eprintln!("muelle: icono del grupo `{id}`: {error:#}"))
                    .ok()
                    .or_else(|| self.placeholder.clone());
                let names: Vec<String> = entry
                    .members
                    .iter()
                    .map(|member| {
                        self.icons
                            .peek(member, "")
                            .map_or_else(|| fallback_name(member), |visual| visual.name.clone())
                    })
                    .collect();
                let label = self
                    .gfx
                    .label_brush(
                        &shorten(&names.join(", "), GROUP_LABEL_CHARS),
                        scale,
                        light,
                        edge,
                    )
                    .inspect_err(|error| eprintln!("muelle: etiqueta del grupo `{id}`: {error:#}"))
                    .ok();
                self.assets.insert(
                    id.clone(),
                    Asset {
                        icon,
                        label,
                        drawn_for: entry.members.clone(),
                        zones: Vec::new(),
                    },
                );
                continue;
            }
            if entry.role == Role::Widget {
                let Some(widget) = crate::widgets::WidgetId::from_entry(id) else {
                    continue;
                };
                let readings = crate::widgets::readings();
                let print = vec![format!("{:?}{}", crate::widgets::dock_size(widget), widget.fingerprint(&readings))];
                if self
                    .assets
                    .get(id)
                    .is_some_and(|asset| asset.drawn_for == print)
                {
                    continue;
                }
                let rendered = crate::widgets::render_cell(&self.gfx, widget, &readings, wanted, light)
                    .inspect_err(|error| eprintln!("muelle: widget `{id}`: {error:#}"))
                    .ok();
                if rendered.as_ref().is_some_and(|(_, changed, _)| *changed) {
                    // Laid out with an estimated width: lay out again.
                    crate::widgets::nudge();
                }
                let (icon, zones) = match rendered {
                    Some((brush, _, zones)) => (Some(brush), shares(widget, wanted, zones)),
                    None => (self.placeholder.clone(), Vec::new()),
                };
                let label = self.gfx.label_brush(widget.name(), scale, light, edge).ok();
                self.assets.insert(
                    id.clone(),
                    Asset {
                        icon,
                        label,
                        drawn_for: print,
                        zones,
                    },
                );
                continue;
            }
            let variant = if id == recycle_bin::PARSING_NAME {
                if self.bin_full {
                    recycle_bin::FULL
                } else {
                    recycle_bin::EMPTY
                }
            } else {
                ""
            };
            let visual = if id == START_ID || window_of(id).is_some() {
                None
            } else {
                self.icons.get(id, variant)
            };
            if self.assets.contains_key(id) {
                continue;
            }
            if let Some(hwnd) = window_of(id) {
                let title = shorten(&tracker::title(hwnd), WINDOW_TITLE_CHARS);
                let label = self.gfx.label_brush(&title, scale, light, edge).ok();
                self.assets.insert(
                    id.clone(),
                    Asset {
                        icon: None,
                        label,
                        drawn_for: Vec::new(),
                        zones: Vec::new(),
                    },
                );
                continue;
            }
            if id == START_ID {
                let icon = self.gfx.start_brush(wanted).ok();
                let label = self.gfx.label_brush(text().start, scale, light, edge).ok();
                self.assets.insert(
                    id.clone(),
                    Asset {
                        icon,
                        label,
                        drawn_for: Vec::new(),
                        zones: Vec::new(),
                    },
                );
                continue;
            }
            // A missing icon should not prevent the rest of the dock from rendering.
            let icon = visual
                .and_then(|visual| {
                    let side = wanted.min(visual.icon.width);
                    self.gfx
                        .icon_brush(&visual.icon, side)
                        .inspect_err(|error| eprintln!("muelle: icono de `{id}`: {error:#}"))
                        .ok()
                })
                .or_else(|| self.placeholder.clone());
            let name = visual.map_or_else(|| fallback_name(id), |visual| visual.name.clone());
            let label = self
                .gfx
                .label_brush(&name, scale, light, edge)
                .inspect_err(|error| eprintln!("muelle: etiqueta de `{id}`: {error:#}"))
                .ok();
            self.assets.insert(
                id.clone(),
                Asset {
                    icon,
                    label,
                    drawn_for: Vec::new(),
                    zones: Vec::new(),
                },
            );
        }
        let entries = &self.entries;
        self.assets
            .retain(|id, _| entries.iter().any(|entry| entry.id.as_ref() == Some(id)));
        let windows = &self.windows;
        let explorer = explorer_id();
        // The apps in groups keep their icons: their group's tile and its
        // unfolded view draw them.
        self.icons.retain(|id| {
            id == explorer
                || entries.iter().any(|entry| {
                    entry.id.as_deref() == Some(id) || entry.members.iter().any(|app| app == id)
                })
                || windows.iter().any(|window| window.app_id == id)
        });
    }

    /// A widget reading changed: repaint the dock tiles that show it, in place.
    pub(crate) fn widgets_changed(&mut self) -> Result<()> {
        let (scale, light, wanted, edge) = self.assets_for;
        if wanted == 0 {
            return Ok(());
        }
        if self.current_entries() != self.entries {
            return self.refresh();
        }
        let readings = crate::widgets::readings();
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.role != Role::Widget {
                continue;
            }
            let Some(id) = entry.id.as_ref() else {
                continue;
            };
            let Some(widget) = crate::widgets::WidgetId::from_entry(id) else {
                continue;
            };
            let print = vec![format!("{:?}{}", crate::widgets::dock_size(widget), widget.fingerprint(&readings))];
            if self
                .assets
                .get(id)
                .is_some_and(|asset| asset.drawn_for == print)
            {
                continue;
            }
            let Ok((icon, resized, zones)) =
                crate::widgets::render_cell(&self.gfx, widget, &readings, wanted, light)
            else {
                continue;
            };
            if resized {
                // A different width: the dock layout changes, not just the picture.
                self.assets.remove(id);
                return self.refresh();
            }
            if let Some(stage) = &self.stage {
                stage.scene.set_slot_brush(index, &icon)?;
            }
            let label = match self.assets.remove(id) {
                Some(asset) => asset.label,
                None => self.gfx.label_brush(widget.name(), scale, light, edge).ok(),
            };
            self.assets.insert(
                id.clone(),
                Asset {
                    icon: Some(icon),
                    label,
                    drawn_for: print,
                    zones: shares(widget, wanted, zones),
                },
            );
        }
        Ok(())
    }

    pub(super) fn assets_ready(&mut self) -> Result<()> {
        let loaded = self.icons.collect();
        if loaded.is_empty() {
            return Ok(());
        }
        for id in &loaded {
            self.assets.remove(id);
            self.names.remove(id);
        }
        // A group's tile and label show its apps' icons and names.
        let groups: Vec<String> = self
            .entries
            .iter()
            .filter(|entry| entry.members.iter().any(|app| loaded.contains(app)))
            .filter_map(|entry| entry.id.clone())
            .collect();
        for id in &groups {
            self.assets.remove(id);
        }
        self.stage = None;
        self.refresh()?;
        self.update_menu_bar_app()
    }
}
