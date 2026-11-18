-- The analytics database: editor and editor_reader read it.
CREATE TABLE visits (
    id int AUTO_INCREMENT PRIMARY KEY,
    page varchar(100) NOT NULL,
    seen_at datetime NOT NULL,
    duration_ms int,
    KEY visits_page (page, seen_at)
);
INSERT INTO visits (page, seen_at, duration_ms)
    SELECT CONCAT('/page/', n % 40), '2024-01-01' + INTERVAL n * 5 MINUTE, n % 9000
    FROM shop.numbers WHERE n <= 3000;
CREATE VIEW daily AS
    SELECT DATE(seen_at) AS day, COUNT(*) AS visits FROM visits GROUP BY DATE(seen_at);
