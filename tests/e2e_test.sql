-- mysql-front-rs end-to-end test script
-- Exercises: statement splitting, comments, DELIMITER, Thai text,
-- NULLs, backslash escapes, foreign keys, views and routines.

DROP DATABASE IF EXISTS mff_test;
CREATE DATABASE mff_test CHARACTER SET utf8mb4 COLLATE utf8mb4_general_ci;
USE mff_test;

CREATE TABLE users (
  id INT UNSIGNED NOT NULL AUTO_INCREMENT,
  name VARCHAR(100) NOT NULL COMMENT 'ชื่อผู้ใช้',
  email VARCHAR(190) DEFAULT NULL,
  score DECIMAL(5,2) DEFAULT NULL,
  bio TEXT COMMENT 'ประวัติ',
  avatar BLOB,
  active TINYINT(1) NOT NULL DEFAULT 1,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (id),
  UNIQUE KEY email (email)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='ตารางผู้ใช้ทดสอบ';

CREATE TABLE orders (
  id INT UNSIGNED NOT NULL AUTO_INCREMENT,
  user_id INT UNSIGNED NOT NULL,
  item VARCHAR(200) NOT NULL,
  amount DECIMAL(10,2) NOT NULL DEFAULT 0.00,
  note VARCHAR(255) DEFAULT NULL,
  PRIMARY KEY (id),
  KEY orders_user (user_id),
  CONSTRAINT fk_orders_user FOREIGN KEY (user_id) REFERENCES users (id)
    ON DELETE CASCADE ON UPDATE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

INSERT INTO users (name, email, score, bio, avatar, active, created_at) VALUES
  ('สมชาย ใจดี', 'somchai@example.com', 88.50, 'หัวหน้าทีม -- ถนัด Rust', 0xDEADBEEF, 1, '2024-01-15 09:30:00'),
  ('Anna O''Brien', 'anna@example.com', 72.00, 'line1\nline2\twith \\backslash', NULL, 1, '2024-02-20 14:05:00'),
  ('สุนิสา', NULL, NULL, NULL, NULL, 0, '2024-03-01 08:00:00'),
  ('Bob; DROP TABLE users;--', 'bob@example.com', 100.00, 'quote''s and "double" quotes', 0x00FF, 1, NOW());

INSERT INTO orders (user_id, item, amount, note) VALUES
  (1, 'Mechanical Keyboard', 2500.00, 'ส่งด่วน'),
  (1, 'Monitor 27"', 8990.50, NULL),
  (2, 'Rust book', 950.00, 'สั่งครั้งที่ 2'),
  (4, 'Coffee', 120.00, NULL);

CREATE VIEW active_users AS
  SELECT id, name, email FROM users WHERE active = 1;

CREATE TRIGGER before_user_insert
  BEFORE INSERT ON users FOR EACH ROW
  SET NEW.created_at = IF(NEW.created_at IS NULL, NOW(), NEW.created_at);

DELIMITER $$

CREATE PROCEDURE count_users(IN min_score DECIMAL(5,2))
BEGIN
  SELECT COUNT(*) AS users_above FROM users WHERE score >= min_score;
  SELECT name, score FROM users WHERE score >= min_score ORDER BY score DESC;
END$$

CREATE FUNCTION user_level(u_score DECIMAL(5,2)) RETURNS VARCHAR(20)
DETERMINISTIC
BEGIN
  IF u_score IS NULL THEN RETURN 'unknown';
  ELSEIF u_score >= 90 THEN RETURN 'expert';
  ELSEIF u_score >= 70 THEN RETURN 'advanced';
  ELSE RETURN 'normal';
  END IF;
END$$

DELIMITER ;

-- Final sanity statement
SELECT 'script finished' AS status;
