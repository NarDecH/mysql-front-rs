CREATE TABLE `users` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `name` varchar(100) NOT NULL COMMENT 'ชื่อผู้ใช้',
  `email` varchar(190) DEFAULT NULL,
  `score` decimal(5,2) DEFAULT NULL,
  `bio` text DEFAULT NULL COMMENT 'ประวัติ',
  `avatar` blob DEFAULT NULL,
  `active` tinyint(1) NOT NULL DEFAULT 1,
  `created_at` datetime NOT NULL DEFAULT current_timestamp(),
  PRIMARY KEY (`id`),
  UNIQUE KEY `email` (`email`)
) ENGINE=InnoDB AUTO_INCREMENT=5 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_general_ci COMMENT='ตารางผู้ใช้ทดสอบ';

INSERT INTO `users` (`id`, `name`, `email`, `score`, `bio`, `avatar`, `active`, `created_at`) VALUES
(1, 'สมชาย ใจดี', 'somchai@example.com', 88.50, 'หัวหน้าทีม -- ถนัด Rust', 0xDEADBEEF, 1, '2024-01-15 09:30:00'),
(2, 'Anna O''Brien', 'anna@example.com', 72.00, 'line1\nline2	with \\backslash', NULL, 1, '2024-02-20 14:05:00'),
(3, 'สุนิสา', NULL, NULL, NULL, NULL, 0, '2024-03-01 08:00:00'),
(4, 'Bob; DROP TABLE users;--', 'bob@example.com', 100.00, 'quote''s and "double" quotes', 0x00FF, 1, '2026-10-28 01:01:24')
;
