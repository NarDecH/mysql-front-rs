#!/usr/bin/env python3
"""Phase 2 of the manual cell context menu: render the floating Area."""
import io

p = "crates/mff-gui/src/data_view.rs"
s = io.open(p, encoding="utf-8").read()

old = '''        // "Insert row" dialog (own window).
        if self
            .sessions
            .get(conn_id)
            .map(|s| s.data.insert_dlg.is_some())
            .unwrap_or(false)
        {
            self.show_insert_dialog(ctx, conn_id);
        }
    }'''

new = '''        // Manual cell context menu (floating Area).
        let menu_state = self.sessions.get(conn_id).and_then(|s| {
            s.data
                .cell_menu
                .map(|(r, c, pos)| (r, c, pos, s.data.fks.clone()))
        });
        if let Some((r, c, pos, fks)) = menu_state {
            let mut pick: Option<CellMenuPick> = None;
            let (col_name, value_text) = self
                .sessions
                .get(conn_id)
                .and_then(|s| s.data.grid.as_ref())
                .and_then(|g| {
                    let name = g.columns.get(c).map(|m| m.name.clone());
                    let val = g.rows.get(r).and_then(|row| row.get(c)).map(cell_text);
                    name.zip(val)
                })
                .unwrap_or_default();
            let fk = fks.iter().find(|f| f.0 == col_name).cloned();

            egui::Area::new(egui::Id::new(("cell_menu", conn_id, r, c)))
                .order(egui::Order::Foreground)
                .fixed_pos(pos)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        if ui.button("⤢ View value").clicked() {
                            pick = Some(CellMenuPick::View);
                        }
                        if let Some((_, ref_table, ref_col)) = &fk {
                            if ui
                                .button(format!("➜ Go to {} ({} …)", ref_table, ref_col))
                                .on_hover_text("Open the referenced row")
                                .clicked()
                            {
                                pick = Some(CellMenuPick::FollowFk);
                            }
                        }
                        if ui.button("⧉ Copy value").clicked() {
                            pick = Some(CellMenuPick::Copy);
                        }
                        if ui.button("Set NULL").clicked() {
                            pick = Some(CellMenuPick::SetNull);
                        }
                        ui.separator();
                        if ui
                            .button(
                                egui::RichText::new("Delete row…")
                                    .color(egui::Color32::LIGHT_RED),
                            )
                            .clicked()
                        {
                            pick = Some(CellMenuPick::Delete);
                        }
                    });
                });

            // Dismiss on any click that did not land on the menu, or Esc.
            let menu_hovered = ctx
                .read_response(egui::Id::new(("cell_menu", conn_id, r, c)))
                .map(|r| r.hovered())
                .unwrap_or(false);
            let dismiss = ctx.input(|i| i.pointer.any_click()) && !menu_hovered;
            let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
            if pick.is_none() && (dismiss || esc) {
                if let Some(state) = self.sessions.get_mut(conn_id) {
                    state.data.cell_menu = None;
                }
            }

            match pick {
                Some(CellMenuPick::View) => {
                    if let Some(state) = self.sessions.get_mut(conn_id) {
                        state.data.cell_view = Some((r, c));
                        state.data.cell_menu = None;
                    }
                }
                Some(CellMenuPick::FollowFk) => {
                    if let Some((_, ref_table, ref_col)) = fk {
                        if value_text != "NULL" {
                            let filter = format!(
                                "{} = {}",
                                mff_core::sqlgen::quote_ident(&ref_col),
                                mff_core::sqlgen::quote_string(&value_text)
                            );
                            if let Some(state) = self.sessions.get_mut(conn_id) {
                                state.data.cell_menu = None;
                            }
                            session_view::open_data_filtered(
                                self,
                                conn_id,
                                ref_table.clone(),
                                Some(filter),
                            );
                            self.status =
                                format!("following {} → {}.{}", ref_col, ref_table, ref_col);
                        } else {
                            if let Some(state) = self.sessions.get_mut(conn_id) {
                                state.data.cell_menu = None;
                            }
                            self.status = "NULL has nothing to follow".into();
                        }
                    }
                }
                Some(CellMenuPick::Copy) => {
                    ctx.copy_text(value_text.clone());
                    self.status = format!("copied: {}", truncate(&value_text, 60));
                    if let Some(state) = self.sessions.get_mut(conn_id) {
                        state.data.cell_menu = None;
                    }
                }
                Some(CellMenuPick::SetNull) => {
                    if let Some(state) = self.sessions.get_mut(conn_id) {
                        state.data.cell_menu = None;
                    }
                    self.apply_cell_edit(conn_id, r, c, "NULL");
                }
                Some(CellMenuPick::Delete) => {
                    if let Some(state) = self.sessions.get_mut(conn_id) {
                        state.data.cell_menu = None;
                        state.data.confirm_delete_row = Some(r);
                    }
                }
                None => {}
            }
        }

        // "Insert row" dialog (own window).
        if self
            .sessions
            .get(conn_id)
            .map(|s| s.data.insert_dlg.is_some())
            .unwrap_or(false)
        {
            self.show_insert_dialog(ctx, conn_id);
        }
    }'''

assert old in s, "insert dialog anchor"
s = s.replace(old, new)

# CellMenuPick enum
old = '''/// Map a display row (after find-filtering) back to its grid row index.
fn display_row(visible: &[usize], display: usize) -> usize {
    visible.get(display).copied().unwrap_or(display)
}'''
new = '''/// Map a display row (after find-filtering) back to its grid row index.
fn display_row(visible: &[usize], display: usize) -> usize {
    visible.get(display).copied().unwrap_or(display)
}

/// What the user picked from the manual cell context menu.
enum CellMenuPick {
    View,
    FollowFk,
    Copy,
    SetNull,
    Delete,
}'''
assert old in s, "enum anchor"
s = s.replace(old, new)

io.open(p, "w", encoding="utf-8", newline="\n").write(s)
print("phase 2 ok")
