# mysql-front-rs

**MySQL-Front เขียนใหม่ด้วยภาษา Rust** — ไคลเอนต์ MySQL แบบ GUI ที่รวดเร็ว ข้ามแพลตฟอร์ม พร้อม CLI ในตัว

> โปรเจ็กต์นี้เป็นการ **ศึกษาและเขียนใหม่ (rewrite)** จากซอร์สโค้ด
> [MySQL-Front 6](https://github.com/StarsunYzj/MySQL-Front) ซึ่งเป็นแอป Delphi/Pascal
> บน Windows (~125,000 บรรทัด) — โครงสร้างโดเมน ฟีเจอร์หลัก และรูปแบบการทำงาน
> อ้างอิงจากโปรเจ็กต์ต้นฉบับ แต่โค้ด Rust ทั้งหมดเขียนขึ้นใหม่
> โดยใช้ไลบรารีมาตรฐานของระบบนิเวศ Rust แทนโค้ด wire-protocol ที่เขียนเองในเวอร์ชันเดิม

---

## สิ่งที่ทำได้ (Features)

### แอป GUI (`mysql-front-rs.exe`)

| ฟีเจอร์ | รายละเอียด |
|---|---|
| จัดการ connections | เพิ่ม/แก้ไข/ทดสอบ/ลบ connection, เก็บในไฟล์ JSON, รองรับ SSL/TLS 4 โหมด |
| Object browser | แสดง databases, tables, views, routines, triggers, events พร้อมช่องกรอง |
| Data browser | ตารางข้อมูลแบบ paginate (เลือกขนาดหน้าได้), sort ด้วยคลิกหัวคอลัมน์, กรองด้วย WHERE, เลือก cell + copy (Ctrl+C) |
| Structure viewer | คอลัมน์, indexes, foreign keys และ `SHOW CREATE TABLE` พร้อมปุ่ม copy / เปิดใน query editor |
| Query editor | หลายแท็บ, รันหลาย statement ต่อครั้ง (แยกสคริปต์ให้อัตโนมัติ, รองรับ `DELIMITER`), แสดงหลาย result set, แสดง rows affected / error, จับเวลา |
| Export | CSV, JSON, SQL (INSERT + CREATE TABLE), HTML, XML จากทั้งตารางและผล query |
| Import | CSV (เลือก delimiter/header), JSON, ไฟล์สคริปต์ .sql |
| Server info | `SHOW VARIABLES` พร้อมช่องกรอง |
| ภาษาไทย | โหลดฟอนต์ไทยของระบบ (Leelawadee UI/Nirmala UI) อัตโนมัติ |

### แอป CLI (`mff-cli.exe`)

```text
mff-cli connections list | add | remove      # จัดการ registry (ใช้ไฟล์เดียวกับ GUI)
mff-cli info       --connection NAME         # ดูข้อมูลเซิร์ฟเวอร์
mff-cli exec       --connection NAME --sql "SELECT ..." [--format table|csv|json]
mff-cli script     --connection NAME --file dump.sql [--continue-on-error]
mff-cli export     --connection NAME --database DB --table T \
                   --format csv|json|sql|html|xml --out FILE [--where "..."] [--limit N]
mff-cli import     --connection NAME --database DB --table T --FILE data.csv
```

### ไลบรารี `mff-core`

ทุกอย่างที่ GUI/CLI ใช้ อยู่ใน crate เดียวที่ reuse ได้ — โมเดล schema, session,
ตัวสร้าง SQL (quoting, DDL, batched INSERT), ตัวแยกสคริปต์ SQL, import/export
พร้อม unit tests 28 ข้อ

---

## การติดตั้งและ build

ต้องมี [Rust](https://rustup.rs) 1.75+ เท่านั้น:

```bash
git clone https://github.com/NarDecH/mysql-front-rs
cd mysql-front-rs
cargo build --release
```

ไฟล์ที่ได้:

* `target/release/mysql-front-rs.exe` — แอป GUI
* `target/release/mff-cli.exe` — แอป CLI

รัน tests:

```bash
cargo test --workspace
```

> **หมายเหตุ (Windows):** build ครั้งแรกใช้เวลาสักครู่เพราะ compile `eframe`/`egui` ทั้งชุด
> สำหรับ Linux ต้องติดตั้ง package ของ `libgtk-3-dev` และ `libudev` ก่อน

## ตำแหน่งที่เก็บ connection

`%APPDATA%\mysql-front\connections.json` (Windows) หรือ
`~/.config/mysql-front/connections.json` (Linux/macOS)

> ⚠️ เหมือนเวอร์ชันต้นฉบับ — **รหัสผ่านถูกเก็บเป็นข้อความล้วน** ในไฟล์นี้
> โปรดรักษาสิทธิ์การเข้าถึงไฟล์ของผู้ใช้ให้ปลอดภัย

---

## สถาปัตยกรรม

```text
mysql-front-rs/
├── crates/
│   ├── mff-core/        # ไลบรารีแกน (ไม่ผูกกับ UI)
│   │   ├── account.rs   #   Connection + Registry (JSON, แทน XML เดิม)
│   │   ├── model.rs     #   Table/Column/Key/ForeignKey/... (แทน uSession.pas)
│   │   ├── session.rs   #   Session บน mysql crate (แทน MySQLClient/MySQLDB.pas)
│   │   ├── sqlgen.rs    #   quoting, CREATE TABLE, batched INSERT (แทน SQLUtils.pas)
│   │   ├── sqlsplit.rs  #   แยกสคริปต์ SQL + DELIMITER (แทน SQLParser.pas เฉพาะส่วนที่ใช้จริง)
│   │   ├── export.rs    #   CSV/JSON/SQL/HTML/XML (แทน TTExport*)
│   │   └── import.rs    #   CSV/JSON/SQL (แทน TTImport*)
│   ├── mff-gui/         # แอปเดสก์ท็อป (eframe/egui) — object browser, data grid, query editor
│   └── mff-cli/         # แอปบรรทัดคำสั่ง (clap)
├── LICENSE              # GPL-3.0-or-later
└── README.md
```

**การเปลี่ยนแปลงสำคัญจากต้นฉบับ**

| ต้นฉบับ (Delphi) | เวอร์ชันนี้ (Rust) |
|---|---|
| wire-protocol เขียนเอง (MySQLClient.pas) | crate [`mysql`](https://crates.io/crates/mysql) (ทดสอบแล้ว, รองรับ auth ใหม่) |
| SQL parser 29k บรรทัด (SQLParser.pas) | ตัวแยก statement ~250 บรรทัดที่จำเป็นต่อการรันสคริปต์ |
| เก็บ connection เป็น XML | JSON ที่อ่านง่าย |
| เก็บตำแหน่งหน้าต่าง/ประวัติ ใน Registry | ยังไม่ได้ทำ (roadmap) |
| รองรับ 28 ภาษา | UI ภาษาอังกฤษ แต่แสดงฟอนต์ไทยได้ |
| ODBC / Access / Excel / PDF | ยังไม่ได้ทำ (roadmap) |

## Roadmap

* แก้ไขข้อมูลใน grid (UPDATE/INSERT/DELETE ราย cell แบบเดิม)
* Table designer แบบแก้ไขได้ / จัดการ users และสิทธิ์
* โอนย้ายข้อมูลระหว่างเซิร์ฟเวอร์ (data transfer) และ backup/restore
* SSH tunnel และ HTTP tunnel (ในต้นฉบับมี HTTPTunnel.pas)
* หลายภาษา (ไฟล์ .ini 28 ภาษาของต้นฉบับเป็นแนวทาง)
* เก็บรหัสผ่านด้วย OS keychain

## License

GPL-3.0-or-later — ดูไฟล์ [LICENSE](LICENSE)

โปรเจ็กต์นี้เป็นผลงานนำมาเขียนใหม่จาก MySQL-Front 6 (Delphi) ซึ่งเผยแพร่ภายใต้ GPL
ขอขอบคุณทีมพัฒนา MySQL-Front เดิมสำหรับดีไซน์และแรงบันดาลใจ
การใด ๆ กับโปรเจ็กต์นี้ต้องเป็นไปตามข้อตกลงของ GPL-3.0 เช่นกัน

---

*(English summary)* **mysql-front-rs** is a Rust rewrite of the MySQL-Front 6
Delphi GUI client: a cross-platform desktop app (egui) plus a CLI, sharing one
core library (`mff-core`) that covers connection management (JSON registry),
schema browsing, data grids with pagination/sorting/filtering, a multi-statement
SQL editor, SQL generation, script splitting, and CSV/JSON/SQL/HTML/XML
import & export. Build with `cargo build --release`; test with
`cargo test --workspace`. Licensed GPL-3.0-or-later.
