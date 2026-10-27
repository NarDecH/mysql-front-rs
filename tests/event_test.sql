CREATE EVENT IF NOT EXISTS mff_test.evt_nightly_cleanup
  ON SCHEDULE EVERY 1 DAY STARTS '2026-11-01 03:00:00'
  DO DELETE FROM mff_test.orders WHERE amount < 0;
