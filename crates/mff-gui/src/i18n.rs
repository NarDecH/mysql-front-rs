//! Tiny i18n: English keys with Thai translations for the main UI chrome.
//! `t(Lang, key)` returns the key itself for English, so untranslated
//! call sites degrade gracefully.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Th,
}

impl Lang {
    pub fn other(self) -> Self {
        match self {
            Lang::En => Lang::Th,
            Lang::Th => Lang::En,
        }
    }

    /// Label of the toggle button (shows the language you switch to).
    pub fn toggle_label(self) -> &'static str {
        match self {
            Lang::En => "🇹🇭 ไทย",
            Lang::Th => "🇬🇧 EN",
        }
    }
}

/// Translate `key` for the language. Unknown keys pass through.
pub fn t(lang: Lang, key: &'static str) -> &'static str {
    match lang {
        Lang::En => key,
        Lang::Th => th(key),
    }
}

fn th(key: &'static str) -> &'static str {
    match key {
        // Tabs.
        "Data" => "ข้อมูล",
        "Structure" => "โครงสร้าง",
        "Query" => "คำสั่ง SQL",
        "Info" => "ข้อมูลเซิร์ฟเวอร์",
        "Server" => "เซิร์ฟเวอร์",
        // Data view toolbar.
        "⟳ Refresh" => "⟳ รีเฟรช",
        "⚒ Structure" => "⚒ โครงสร้าง",
        "⭳ Export…" => "⭳ ส่งออก…",
        "⇪ Import…" => "⇪ นำเข้า…",
        "＋ Row" => "＋ เพิ่มแถว",
        "⤢ View cell" => "⤢ ดูค่าเต็ม",
        "filter (WHERE):" => "กรอง (WHERE):",
        "Apply" => "ใช้",
        "✕ Clear" => "✕ ล้าง",
        "find:" => "ค้นหา:",
        "go" => "ไป",
        "page size:" => "จำนวน/หน้า:",
        // Query view.
        "▶ Run (Ctrl+Enter)" => "▶ รัน (Ctrl+Enter)",
        "✨ Format" => "✨ จัดรูปแบบ",
        "🔍 Explain" => "🔍 วิเคราะห์แผน",
        "✕ Clear results" => "✕ ล้างผลลัพธ์",
        "🕘 History" => "🕘 ประวัติ",
        // Top bars / menu.
        "⏻ Disconnect" => "⏻ ตัดการเชื่อมต่อ",
        "Menu ▾" => "เมนู ▾",
        "⟳ Refresh" => "⟳ รีเฟรช",
        "database:" => "ฐานข้อมูล:",
        "Rename database…" => "เปลี่ยนชื่อฐานข้อมูล…",
        "Transfer data…" => "ย้ายข้อมูล…",
        "Processes & users" => "โปรเซสและผู้ใช้",
        "Backup database…" => "สำรองฐานข้อมูล…",
        "Restore database…" => "กู้คืนฐานข้อมูล…",
        "Compare table with…" => "เทียบข้อมูลตารางกับ…",
        // Home screen.
        "＋ New connection" => "＋ การเชื่อมต่อใหม่",
        "Connect" => "เชื่อมต่อ",
        "Edit" => "แก้ไข",
        "Duplicate" => "ทำสำเนา",
        "Delete" => "ลบ",
        "Name" => "ชื่อ",
        "Host:port" => "โฮสต์:พอร์ต",
        "User" => "ผู้ใช้",
        "Database" => "ฐานข้อมูล",
        // Status bar.
        "⌂ Connections" => "⌂ หน้าเชื่อมต่อ",
        "⧉ Copy cell" => "⧉ คัดลอกค่า",
        // Session tabs strip.
        "＋" => "＋",
        _ => key,
    }
}
