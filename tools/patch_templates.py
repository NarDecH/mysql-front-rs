#!/usr/bin/env python3
"""Add right-click 'New …' templates to the object-browser group headers."""
import io

p = r"crates\mff-gui\src\session_view.rs"
src = io.open(p, encoding="utf-8").read()


def rep(old, new, tag):
    global src
    assert old in src, f"pattern not found: {tag}"
    src = src.replace(old, new, 1)


# 1. New TreeAction variant.
rep(
    """enum TreeAction {
    Select(ObjKind, String),
    Open(ObjKind, String),
    ShowDef(ObjKind, String, String),""",
    """enum TreeAction {
    Select(ObjKind, String),
    Open(ObjKind, String),
    ShowDef(ObjKind, String, String),
    ShowTemplate(String, String),""",
    "enum",
)

# 2. Tables: capture response, context menu after the collapsing body.
rep(
    '''                    ui.collapsing(format!("Tables ({})", objects.tables.len()), |ui| {''',
    '''                    let tables_group = ui.collapsing(
                        format!("Tables ({})", objects.tables.len()),
                        |ui| {''',
    "tables-open",
)
rep(
    """                            });
                        }
                    });
                    ui.collapsing(format!("Views ({})", objects.views.len()), |ui| {""",
    """                            });
                        }
                    });
                    tables_group
                        .header_response
                        .context_menu(|ui| {
                            if ui.button("New table…").clicked() {
                                ui.close();
                                actions.push(TreeAction::ShowTemplate(
                                    "New table".into(),
                                    r#"CREATE TABLE `new_table` (
  `id` INT UNSIGNED NOT NULL AUTO_INCREMENT,
  `name` VARCHAR(100) NOT NULL,
  `created_at` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;"#
                                        .into(),
                                ));
                            }
                        });
                    ui.collapsing(format!("Views ({})", objects.views.len()), |ui| {""",
    "tables-close",
)

# 3. Views.
rep(
    '''                    ui.collapsing(format!("Views ({})", objects.views.len()), |ui| {''',
    '''                    let views_group = ui.collapsing(
                        format!("Views ({})", objects.views.len()),
                        |ui| {''',
    "views-open",
)
rep(
    """                            if resp.clicked() || resp.double_clicked() {
                                actions.push(TreeAction::Open(ObjKind::View, v.name.clone()));
                            }
                        }
                    });
                    ui.collapsing(format!("Routines ({})", objects.routines.len()), |ui| {""",
    """                            if resp.clicked() || resp.double_clicked() {
                                actions.push(TreeAction::Open(ObjKind::View, v.name.clone()));
                            }
                        }
                    });
                    views_group
                        .header_response
                        .context_menu(|ui| {
                            if ui.button("New view…").clicked() {
                                ui.close();
                                actions.push(TreeAction::ShowTemplate(
                                    "New view".into(),
                                    r#"CREATE OR REPLACE VIEW `new_view` AS
SELECT `id`, `name`
FROM `users`
WHERE `active` = 1;"#
                                        .into(),
                                ));
                            }
                        });
                    ui.collapsing(format!("Routines ({})", objects.routines.len()), |ui| {""",
    "views-close",
)

# 4. Routines.
rep(
    '''                    ui.collapsing(format!("Routines ({})", objects.routines.len()), |ui| {''',
    '''                    let routines_group = ui.collapsing(
                        format!("Routines ({})", objects.routines.len()),
                        |ui| {''',
    "routines-open",
)
rep(
    """                                actions.push(TreeAction::ShowDef(
                                    ObjKind::Routine,
                                    r.name.clone(),
                                    sql,
                                ));
                            }
                        }
                    });
                    ui.collapsing(format!("Triggers ({})", objects.triggers.len()), |ui| {""",
    """                                actions.push(TreeAction::ShowDef(
                                    ObjKind::Routine,
                                    r.name.clone(),
                                    sql,
                                ));
                            }
                        }
                    });
                    routines_group
                        .header_response
                        .context_menu(|ui| {
                            if ui.button("New procedure…").clicked() {
                                ui.close();
                                actions.push(TreeAction::ShowTemplate(
                                    "New procedure".into(),
                                    r#"DELIMITER $$

CREATE PROCEDURE `new_procedure`(IN p_id INT UNSIGNED)
BEGIN
  SELECT * FROM `users` WHERE `id` = p_id;
END$$

DELIMITER ;"#
                                        .into(),
                                ));
                            }
                            if ui.button("New function…").clicked() {
                                ui.close();
                                actions.push(TreeAction::ShowTemplate(
                                    "New function".into(),
                                    r#"DELIMITER $$

CREATE FUNCTION `new_function`(p_score DECIMAL(5,2))
RETURNS VARCHAR(20)
DETERMINISTIC
BEGIN
  IF p_score IS NULL THEN RETURN 'unknown';
  ELSE RETURN 'ok';
  END IF;
END$$

DELIMITER ;"#
                                        .into(),
                                ));
                            }
                        });
                    ui.collapsing(format!("Triggers ({})", objects.triggers.len()), |ui| {""",
    "routines-close",
)

# 5. Triggers.
rep(
    '''                    ui.collapsing(format!("Triggers ({})", objects.triggers.len()), |ui| {''',
    '''                    let triggers_group = ui.collapsing(
                        format!("Triggers ({})", objects.triggers.len()),
                        |ui| {''',
    "triggers-open",
)
rep(
    """                                actions.push(TreeAction::ShowDef(
                                    ObjKind::Trigger,
                                    t.name.clone(),
                                    sql,
                                ));
                            }
                        }
                    });
                    ui.collapsing(format!("Events ({})", objects.events.len()), |ui| {""",
    """                                actions.push(TreeAction::ShowDef(
                                    ObjKind::Trigger,
                                    t.name.clone(),
                                    sql,
                                ));
                            }
                        }
                    });
                    triggers_group
                        .header_response
                        .context_menu(|ui| {
                            if ui.button("New trigger…").clicked() {
                                ui.close();
                                actions.push(TreeAction::ShowTemplate(
                                    "New trigger".into(),
                                    r#"CREATE TRIGGER `new_trigger`
BEFORE INSERT ON `users` FOR EACH ROW
SET NEW.`created_at` = NOW();"#
                                        .into(),
                                ));
                            }
                        });
                    ui.collapsing(format!("Events ({})", objects.events.len()), |ui| {""",
    "triggers-close",
)

# 6. Events.
rep(
    '''                    ui.collapsing(format!("Events ({})", objects.events.len()), |ui| {''',
    '''                    let events_group = ui.collapsing(
                        format!("Events ({})", objects.events.len()),
                        |ui| {''',
    "events-open",
)
rep(
    """                                actions.push(TreeAction::ShowDef(
                                    ObjKind::Event,
                                    e.name.clone(),
                                    sql,
                                ));
                            }
                        }
                    });
                });
            });""",
    """                                actions.push(TreeAction::ShowDef(
                                    ObjKind::Event,
                                    e.name.clone(),
                                    sql,
                                ));
                            }
                        }
                    });
                    events_group
                        .header_response
                        .context_menu(|ui| {
                            if ui.button("New event…").clicked() {
                                ui.close();
                                actions.push(TreeAction::ShowTemplate(
                                    "New event".into(),
                                    r#"CREATE EVENT `new_event`
ON SCHEDULE EVERY 1 DAY
DO
  DELETE FROM `orders` WHERE `amount` < 0;"#
                                        .into(),
                                ));
                            }
                        });
                });
            });""",
    "events-close",
)

# 7. Deferred ShowTemplate: fresh query tab (unique title), no auto-run.
rep(
    """                TreeAction::StartRename(kind, name) => {""",
    """                TreeAction::ShowTemplate(title, sql) => {
                    let idx = {
                        let Some(state) = self.sessions.get_mut(&conn_id) else {
                            continue;
                        };
                        state.view = ActiveView::Query;
                        let mut n = 1;
                        let mut unique = title.clone();
                        while state.queries.iter().any(|q| q.title == unique) {
                            n += 1;
                            unique = format!("{title} {n}");
                        }
                        let mut tab = QueryTab::new(unique);
                        tab.code = sql;
                        state.queries.push(tab);
                        state.active_query = state.queries.len() - 1;
                        state.queries.len() - 1
                    };
                    let _ = idx;
                }
                TreeAction::StartRename(kind, name) => {""",
    "deferred",
)

io.open(p, "w", encoding="utf-8", newline="\n").write(src)
print("templates patch applied")
