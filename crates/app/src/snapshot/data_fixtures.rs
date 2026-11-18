use std::path::{Path, PathBuf};

use database::{Cancel, Collect, DbError, Secret, Target};

/// A SQLite database (customers, their orders, products, a view) and a CSV file, made
/// in `dir` (emptied first).
pub fn write(dir: &Path) -> Result<(PathBuf, PathBuf), String> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let db = dir.join("shop.sqlite");
    sqlite(&db).map_err(|e| e.to_string())?;
    let csv = dir.join("people.csv");
    std::fs::write(&csv, people()).map_err(|e| e.to_string())?;
    Ok((db, csv))
}

const COUNTRIES: [&str; 5] = ["Romania", "Germany", "France", "Italy", "Spain"];
const NAMES: [&str; 12] = [
    "Ana Popescu",
    "Bogdan Ionescu",
    "Clara Weber",
    "Dan Marin",
    "Elena Rossi",
    "Florin Dima",
    "Greta Klein",
    "Horia Stan",
    "Irina Lupu",
    "Jean Martin",
    "Karla Ruiz",
    "Luca Bianchi",
];
const STATUSES: [&str; 4] = ["new", "paid", "shipped", "cancelled"];

fn sqlite(path: &Path) -> Result<(), DbError> {
    let factory = database::drivers::find("sqlite")
        .ok_or_else(|| DbError::Unsupported("the SQLite driver".to_owned()))?;
    let mut target = Target { database: path.to_string_lossy().into_owned(), ..Target::default() };
    target.options.insert("create".to_owned(), "true".to_owned());
    let mut session = (factory.create)().connect(&target, &Secret::new(""))?;
    let mut run = |sql: &str| -> Result<(), DbError> {
        session.execute(sql, &mut Collect::default(), &Cancel::default()).map(|_| ())
    };
    run(
        "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT UNIQUE, country TEXT, created TEXT)",
    )?;
    run(
        "CREATE TABLE orders (id INTEGER PRIMARY KEY AUTOINCREMENT, customer_id INTEGER REFERENCES customers(id) ON DELETE CASCADE, total REAL NOT NULL, status TEXT DEFAULT 'new', note TEXT)",
    )?;
    run("CREATE INDEX orders_customer ON orders (customer_id)")?;
    run(
        "CREATE TABLE products (sku TEXT PRIMARY KEY, title TEXT NOT NULL, price NUMERIC, stock INTEGER, image BLOB)",
    )?;
    run("CREATE VIEW big_orders AS SELECT * FROM orders WHERE total > 100")?;
    run("BEGIN")?;
    for (i, name) in NAMES.iter().enumerate() {
        let email = name.to_lowercase().replace(' ', ".") + "@example.com";
        let email = if i == 7 { "NULL".to_owned() } else { format!("'{email}'") };
        run(&format!(
            "INSERT INTO customers VALUES ({}, '{name}', {email}, '{}', '2026-0{}-1{} 10:00:00')",
            i + 1,
            COUNTRIES[i % COUNTRIES.len()],
            i % 9 + 1,
            i % 10
        ))?;
    }
    for i in 0..40u32 {
        let total = f64::from((i * 37 + 11) % 180) + f64::from(i % 4) * 0.25;
        let note = match i % 5 {
            0 => "NULL".to_owned(),
            1 => "'gift wrap'".to_owned(),
            2 => "'call before delivery, ring twice'".to_owned(),
            _ => "''".to_owned(),
        };
        run(&format!(
            "INSERT INTO orders (customer_id, total, status, note) VALUES ({}, {total}, '{}', {note})",
            i % 12 + 1,
            STATUSES[(i as usize * 3) % STATUSES.len()]
        ))?;
    }
    let products = [
        ("KB-01", "Mechanical keyboard", "89.90", "14"),
        ("MS-02", "Wireless mouse", "29.50", "40"),
        ("MN-27", "Monitor 27\"", "319.00", "6"),
        ("HD-01", "Headphones", "NULL", "0"),
        ("CB-10", "USB-C cable", "9.99", "120"),
        ("DK-04", "Docking station", "149.00", "NULL"),
        ("WC-01", "Webcam", "59.00", "22"),
        ("LP-15", "Laptop stand", "39.90", "9"),
    ];
    for (sku, title, price, stock) in products {
        run(&format!(
            "INSERT INTO products VALUES ('{sku}', '{}', {price}, {stock}, X'89504e47')",
            title.replace('\'', "''")
        ))?;
    }
    run("COMMIT")
}

/// A store's SQLite database (`store.sqlite`, made in `dir`, emptied first): users,
/// their profiles, orders and their lines, products in categories (a tree) — joined by
/// foreign keys — and reviews, payments and shipments that only name what they point to
/// (`productId`, `userid`, `order_id`, `fk_order`: no key), with columns that look like
/// keys and aren't (`paid`, `status_id` without a status table).
pub fn store(dir: &Path) -> Result<PathBuf, String> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join("store.sqlite");
    let factory = database::drivers::find("sqlite").ok_or("the SQLite driver")?;
    let mut target = Target { database: path.to_string_lossy().into_owned(), ..Target::default() };
    target.options.insert("create".to_owned(), "true".to_owned());
    let mut session =
        (factory.create)().connect(&target, &Secret::new("")).map_err(|e| e.to_string())?;
    let statements = [
        "CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT NOT NULL UNIQUE, name TEXT, created_at TEXT)",
        "CREATE TABLE profiles (user_id INTEGER PRIMARY KEY REFERENCES users(id), bio TEXT, avatar BLOB)",
        "CREATE TABLE categories (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES categories(id), name TEXT NOT NULL)",
        "CREATE TABLE products (id INTEGER PRIMARY KEY, category_id INTEGER REFERENCES categories(id), sku TEXT UNIQUE, title TEXT NOT NULL, price NUMERIC)",
        "CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL REFERENCES users(id), status TEXT, total NUMERIC, created_at TEXT)",
        "CREATE TABLE order_items (order_id INTEGER REFERENCES orders(id), product_id INTEGER REFERENCES products(id), qty INTEGER, price NUMERIC, PRIMARY KEY (order_id, product_id))",
        "CREATE TABLE reviews (id INTEGER PRIMARY KEY, productId INTEGER, userid INTEGER, rating INTEGER, body TEXT)",
        "CREATE TABLE payments (id INTEGER PRIMARY KEY, order_id INTEGER, amount NUMERIC, paid INTEGER, method TEXT)",
        "CREATE TABLE shipments (id INTEGER PRIMARY KEY, fk_order INTEGER, status_id INTEGER, carrier TEXT)",
        "INSERT INTO users VALUES (1, 'ana@example.com', 'Ana Popescu', '2026-01-10')",
        "INSERT INTO orders VALUES (1, 1, 'paid', 119.87, '2026-02-01')",
    ];
    for sql in statements {
        session
            .execute(sql, &mut Collect::default(), &Cancel::default())
            .map_err(|e| e.to_string())?;
    }
    Ok(path)
}

/// A SQLite database of events with JSON payloads, made in `dir`.
pub fn events(dir: &Path) -> Result<PathBuf, String> {
    let path = dir.join("events.sqlite");
    let _ = std::fs::remove_file(&path);
    let factory = database::drivers::find("sqlite").ok_or("the SQLite driver")?;
    let mut target = Target { database: path.to_string_lossy().into_owned(), ..Target::default() };
    target.options.insert("create".to_owned(), "true".to_owned());
    let mut session =
        (factory.create)().connect(&target, &Secret::new("")).map_err(|e| e.to_string())?;
    let mut run =
        |sql: &str| session.execute(sql, &mut Collect::default(), &Cancel::default()).map(|_| ());
    run("CREATE TABLE events (id INTEGER PRIMARY KEY, payload JSON, at DATETIME)")
        .map_err(|e| e.to_string())?;
    let payloads = [
        r#"{"type":"signup","user":{"id":7,"name":"Ana Popescu"},"plan":"pro"}"#,
        r#"{"type":"order","order":{"id":1042,"items":[{"sku":"KB-01","qty":1},{"sku":"CB-10","qty":3}],"total":119.87},"paid":true}"#,
        r#"{"type":"login","user":{"id":3},"agent":"Firefox","mfa":false}"#,
        r#"{"type":"refund","order":1042,"amount":9.99,"reason":null}"#,
    ];
    for (i, payload) in payloads.iter().enumerate() {
        run(&format!(
            "INSERT INTO events VALUES ({}, '{payload}', '2026-10-0{} 09:1{}:00')",
            i + 1,
            i + 1,
            i
        ))
        .map_err(|e| e.to_string())?;
    }
    Ok(path)
}

fn people() -> String {
    let mut out = String::from("id,name,city,score,joined,active\n");
    let cities = ["Cluj", "Iasi", "Berlin", "Lyon", "Milano", "Madrid"];
    for i in 0..30usize {
        let name = super::data_fixtures::NAMES[i % 12];
        let score =
            if i % 7 == 3 { String::new() } else { format!("{}.{}", 40 + (i * 13) % 60, i % 10) };
        out.push_str(&format!(
            "{},{},{},{score},2025-{:02}-{:02},{}\n",
            i + 1,
            if i == 4 { format!("\"{name}, Jr.\"") } else { name.to_owned() },
            cities[i % cities.len()],
            i % 12 + 1,
            i % 27 + 1,
            if i % 3 == 0 { "false" } else { "true" }
        ));
    }
    out
}
