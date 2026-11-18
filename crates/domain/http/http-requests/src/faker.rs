//! Faker variables (plan 77j): `{{$random.<class>.<method>}}` — `name.fullName`,
//! `address.city`, `internet.emailAddress`… — every class its documentation lists, each
//! method that takes no parameter, from word lists of our own; with a locale and a seed,
//! `{{$random.locale.name.fullName("fr", 12)}}`, the same seed giving the same value (the
//! words are English whatever the locale: the lists are ours, one language).

/// The value of Faker `class.method` (its case as written or lower), drawing from `next`;
/// `None`: no such one.
pub fn fake(class: &str, method: &str, next: &mut dyn FnMut() -> u64) -> Option<String> {
    let class = class.to_ascii_lowercase();
    let method = method.to_ascii_lowercase();
    let value = match (class.as_str(), method.as_str()) {
        ("name", "firstname") => pick(next, FIRST),
        ("name", "lastname") => pick(next, LAST),
        ("name", "fullname" | "name" | "namewithmiddle") => {
            let first = pick(next, FIRST);
            format!("{first} {}", pick(next, LAST))
        }
        ("name", "prefix") => pick(next, &["Mr.", "Mrs.", "Ms.", "Miss", "Dr."]),
        ("name", "suffix") => pick(next, &["Jr.", "Sr.", "I", "II", "III", "PhD", "MD"]),
        ("name", "title") => {
            let level = pick(next, &["Senior", "Lead", "Junior", "Principal", "Chief"]);
            format!("{level} {}", pick(next, JOBS))
        }
        ("name", "username") => {
            let first = pick(next, FIRST).to_lowercase();
            format!("{first}.{}", pick(next, LAST).to_lowercase())
        }
        ("address", "city" | "cityname") => pick(next, CITIES),
        ("address", "country") => pick(next, COUNTRIES),
        ("address", "countrycode") => {
            pick(next, &["US", "GB", "DE", "FR", "RO", "IT", "ES", "NL", "SE", "JP"])
        }
        ("address", "state") => {
            pick(next, &["California", "Texas", "Ohio", "Oregon", "Florida", "Nevada"])
        }
        ("address", "stateabbr") => pick(next, &["CA", "TX", "OH", "OR", "FL", "NV"]),
        ("address", "streetname") => {
            let street = pick(next, LAST);
            format!("{street} {}", pick(next, &["Street", "Avenue", "Road", "Lane", "Boulevard"]))
        }
        ("address", "buildingnumber") => (1 + next() % 999).to_string(),
        ("address", "streetaddress") => {
            let number = 1 + next() % 999;
            let street = pick(next, LAST);
            format!("{number} {street} {}", pick(next, &["Street", "Avenue", "Road", "Lane"]))
        }
        ("address", "zipcode" | "postcode") => format!("{:05}", next() % 100_000),
        ("address", "fulladdress") => {
            let number = 1 + next() % 999;
            let street = pick(next, LAST);
            let city = pick(next, CITIES);
            format!("{number} {street} Street, {city} {:05}", next() % 100_000)
        }
        ("address", "latitude") => format!("{:.6}", (next() % 180_000_000) as f64 / 1e6 - 90.0),
        ("address", "longitude") => format!("{:.6}", (next() % 360_000_000) as f64 / 1e6 - 180.0),
        ("address", "timezone") => {
            pick(next, &["Europe/Bucharest", "Europe/London", "America/New_York", "Asia/Tokyo"])
        }
        ("beer", "name") => {
            pick(next, &["Hop Storm", "Old Oak Ale", "Night Porter", "Golden Wheat", "Red Fox IPA"])
        }
        ("beer", "style") => pick(next, &["IPA", "Stout", "Porter", "Pilsner", "Lager", "Saison"]),
        ("beer", "hop") => pick(next, &["Cascade", "Citra", "Mosaic", "Saaz", "Simcoe"]),
        ("beer", "malt") => pick(next, &["Pilsner", "Munich", "Vienna", "Crystal", "Chocolate"]),
        ("beer", "yeast") => pick(next, &["Ale", "Lager", "Wheat", "Belgian", "Saison"]),
        ("bool", "bool") => pick(next, &["true", "false"]),
        ("business", "creditcardnumber") => card(next),
        ("business", "creditcardtype") => pick(next, &["visa", "mastercard", "amex", "discover"]),
        ("business", "creditcardexpiry") => format!("{:02}/{}", 1 + next() % 12, 2027 + next() % 6),
        ("chucknorris", "fact") => pick(
            next,
            &[
                "Chuck Norris can divide by zero.",
                "Chuck Norris writes code that optimizes itself.",
                "Chuck Norris's keyboard has no Ctrl key: he is always in control.",
            ],
        ),
        ("code", "isbn10") => digits(next, 10),
        ("code", "isbn13") => format!("978{}", digits(next, 10)),
        ("code", "ean8") => digits(next, 8),
        ("code", "ean13") => digits(next, 13),
        ("code", "asin") => format!("B0{}", upper(next, 8)),
        ("color", "name") => pick(next, COLORS),
        ("color", "hex") => format!("#{:06x}", next() % 0x100_0000),
        ("commerce", "productname") => {
            let quality =
                pick(next, &["Small", "Ergonomic", "Rustic", "Sleek", "Durable", "Awesome"]);
            let material = pick(next, MATERIALS);
            format!("{quality} {material} {}", pick(next, PRODUCTS))
        }
        ("commerce", "department") => {
            pick(next, &["Books", "Tools", "Garden", "Music", "Toys", "Sports"])
        }
        ("commerce", "color") => pick(next, COLORS),
        ("commerce", "material") => pick(next, MATERIALS),
        ("commerce", "price") => format!("{}.{:02}", 1 + next() % 999, next() % 100),
        ("commerce", "promotioncode") => {
            format!("{}{}", pick(next, &["Sale", "Deal", "Promo", "Gift"]), 10 + next() % 90)
        }
        ("company", "name") => {
            let name = pick(next, LAST);
            format!("{name} {}", pick(next, &["Inc", "LLC", "Group", "and Sons", "Ltd"]))
        }
        ("company", "suffix") => pick(next, &["Inc", "LLC", "Group", "Ltd", "SRL"]),
        ("company", "industry") => {
            pick(next, &["Software", "Banking", "Retail", "Logistics", "Health Care"])
        }
        ("company", "profession") => pick(next, JOBS),
        ("company", "buzzword") => pick(next, BUZZ),
        ("company", "catchphrase") => {
            let first = pick(next, BUZZ);
            format!(
                "{first} {}",
                pick(next, &["platform", "solution", "framework", "service", "pipeline"])
            )
        }
        ("company", "bs") => {
            let verb = pick(next, &["leverage", "streamline", "empower", "synergize", "scale"]);
            format!("{verb} {}", pick(next, BUZZ).to_lowercase())
        }
        ("company", "url") => format!("https://www.{}.com", pick(next, LAST).to_lowercase()),
        ("crypto", "md5") => {
            let seed = digits(next, 12);
            crate::hash::hex(&crate::hash::digest(crate::hash::Algorithm::Md5, seed.as_bytes()))
        }
        ("crypto", "sha1") => {
            let seed = digits(next, 12);
            crate::hash::hex(&crate::hash::digest(crate::hash::Algorithm::Sha1, seed.as_bytes()))
        }
        ("crypto", "sha256") => {
            let seed = digits(next, 12);
            crate::hash::hex(&crate::hash::digest(crate::hash::Algorithm::Sha256, seed.as_bytes()))
        }
        ("crypto", "sha512") => {
            let seed = digits(next, 12);
            crate::hash::hex(&crate::hash::digest(crate::hash::Algorithm::Sha512, seed.as_bytes()))
        }
        ("educator", "university") => {
            let city = pick(next, CITIES);
            format!("University of {city}")
        }
        ("educator", "course") => {
            pick(next, &["Algorithms", "Databases", "Networks", "Compilers", "Statistics"])
        }
        ("educator", "campus") => format!("{} Campus", pick(next, CITIES)),
        ("finance", "iban") => format!("RO{:02}BANK{}", next() % 100, digits(next, 16)),
        ("finance", "bic") => format!("{}RO{}", upper(next, 4), upper(next, 2)),
        ("finance", "creditcard") => card(next),
        ("hacker", "abbreviation") => {
            pick(next, &["TCP", "HTTP", "SQL", "JSON", "SSL", "API", "XML"])
        }
        ("hacker", "adjective") => {
            pick(next, &["virtual", "wireless", "redundant", "open-source", "neural"])
        }
        ("hacker", "noun") => {
            pick(next, &["driver", "protocol", "bandwidth", "matrix", "firewall"])
        }
        ("hacker", "verb") => pick(next, &["parse", "compress", "index", "bypass", "synthesize"]),
        ("hacker", "ingverb") => pick(next, &["parsing", "compressing", "indexing", "bypassing"]),
        ("idnumber", "valid" | "ssnvalid") => {
            format!("{:03}-{:02}-{:04}", 100 + next() % 800, 10 + next() % 90, 1000 + next() % 9000)
        }
        ("idnumber", "invalid") => format!("000-{:02}-{:04}", next() % 100, next() % 10_000),
        ("internet", "emailaddress" | "safeemailaddress") => {
            let first = pick(next, FIRST).to_lowercase();
            let last = pick(next, LAST).to_lowercase();
            format!("{first}.{last}@{}", pick(next, &["example.com", "example.org", "example.net"]))
        }
        ("internet", "domainname") => format!("{}.com", pick(next, LAST).to_lowercase()),
        ("internet", "domainword") => pick(next, LAST).to_lowercase(),
        ("internet", "domainsuffix") => pick(next, &["com", "org", "net", "io", "ro"]),
        ("internet", "url") => format!("https://www.{}.com/", pick(next, LAST).to_lowercase()),
        ("internet", "slug") => {
            let first = pick(next, LOREM);
            format!("{first}-{}", pick(next, LOREM))
        }
        ("internet", "password") => alnum(next, 12),
        ("internet", "ipv4address" | "publicipv4address") => {
            format!("{}.{}.{}.{}", 1 + next() % 223, next() % 256, next() % 256, 1 + next() % 254)
        }
        ("internet", "privateipv4address") => {
            format!("192.168.{}.{}", next() % 256, 1 + next() % 254)
        }
        ("internet", "ipv6address") => {
            (0..8).map(|_| format!("{:x}", next() % 0x10000)).collect::<Vec<_>>().join(":")
        }
        ("internet", "macaddress") => {
            (0..6).map(|_| format!("{:02x}", next() % 256)).collect::<Vec<_>>().join(":")
        }
        ("internet", "uuid") => {
            let hex: Vec<u64> = (0..4).map(|_| next()).collect();
            format!(
                "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
                hex[0] as u32,
                hex[1] as u16,
                hex[1] >> 52,
                0x8000 | (hex[2] as u16 & 0x3fff),
                hex[3] & 0xffff_ffff_ffff
            )
        }
        ("internet", "useragent" | "useragentany") => pick(
            next,
            &[
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36",
            ],
        ),
        ("lorem", "word") => pick(next, LOREM),
        ("lorem", "character") => alnum(next, 1).to_lowercase(),
        ("lorem", "characters") => alnum(next, 16).to_lowercase(),
        ("lorem", "sentence") => sentence(next, 8),
        ("lorem", "paragraph") => (0..4).map(|_| sentence(next, 8)).collect::<Vec<_>>().join(" "),
        ("number", "digit" | "randomdigit") => (next() % 10).to_string(),
        ("number", "randomdigitnotzero") => (1 + next() % 9).to_string(),
        ("number", "randomnumber") => (next() % 1_000_000_000).to_string(),
        ("number", "randomdouble") => format!("{:.2}", (next() % 1_000_000) as f64 / 100.0),
        ("phonenumber", "phonenumber" | "cellphone" | "subscribernumber") => {
            format!("+40 7{:02} {:03} {:03}", next() % 100, next() % 1000, next() % 1000)
        }
        ("phonenumber", "extension") => (100 + next() % 900).to_string(),
        (
            "shakespeare",
            "hamletquote" | "asyoulikeitquote" | "kingrichardiiiquote" | "romeoandjulietquote",
        ) => pick(
            next,
            &[
                "To be, or not to be: that is the question.",
                "All the world's a stage.",
                "Now is the winter of our discontent.",
                "What's in a name? That which we call a rose by any other name would smell as sweet.",
            ],
        ),
        ("superhero", "name") => {
            let first = pick(next, &["Captain", "Doctor", "Iron", "Silver", "Night"]);
            format!("{first} {}", pick(next, &["Falcon", "Storm", "Shadow", "Comet", "Titan"]))
        }
        ("superhero", "power") => {
            pick(next, &["Flight", "Invisibility", "Super Strength", "Telepathy"])
        }
        ("superhero", "prefix") => pick(next, &["Captain", "Doctor", "Iron", "Silver", "Night"]),
        ("superhero", "suffix") => pick(next, &["Falcon", "Storm", "Shadow", "Comet", "Titan"]),
        ("team", "name") => {
            let city = pick(next, CITIES);
            format!("{city} {}", pick(next, &["Wolves", "Eagles", "Lions", "Rangers", "Comets"]))
        }
        ("team", "sport") => pick(next, &["football", "basketball", "tennis", "hockey", "rugby"]),
        ("team", "creature") => pick(next, &["wolves", "eagles", "lions", "bears", "dragons"]),
        ("team", "state") => pick(next, &["California", "Texas", "Ohio", "Oregon"]),
        ("university", "name") => {
            let city = pick(next, CITIES);
            format!("University of {city}")
        }
        ("university", "prefix") => pick(next, &["North", "South", "East", "West", "Central"]),
        ("university", "suffix") => pick(next, &["University", "Institute", "College", "Academy"]),
        _ => return None,
    };
    Some(value)
}

/// A seeded source of numbers (`{{$random.locale.name.fullName("fr", 12)}}`): the same seed,
/// the same numbers.
pub fn seeded(seed: u64) -> impl FnMut() -> u64 {
    let mut state = seed ^ 0x9E37_79B9_7F4A_7C15;
    move || {
        // SplitMix64.
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

fn pick(next: &mut dyn FnMut() -> u64, list: &[&str]) -> String {
    list[(next() % list.len() as u64) as usize].to_owned()
}

fn digits(next: &mut dyn FnMut() -> u64, n: usize) -> String {
    (0..n).map(|_| char::from(b'0' + (next() % 10) as u8)).collect()
}

fn upper(next: &mut dyn FnMut() -> u64, n: usize) -> String {
    (0..n).map(|_| char::from(b'A' + (next() % 26) as u8)).collect()
}

fn alnum(next: &mut dyn FnMut() -> u64, n: usize) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    (0..n).map(|_| char::from(CHARS[(next() % CHARS.len() as u64) as usize])).collect()
}

/// A card number that passes the Luhn check (16 digits, a Visa's `4` first).
fn card(next: &mut dyn FnMut() -> u64) -> String {
    let mut digits: Vec<u32> =
        std::iter::once(4).chain((0..14).map(|_| (next() % 10) as u32)).collect();
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, d)| {
            if i % 2 == 0 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                *d
            }
        })
        .sum();
    digits.push((10 - sum % 10) % 10);
    digits.iter().map(|d| char::from_digit(*d, 10).unwrap_or('0')).collect()
}

fn sentence(next: &mut dyn FnMut() -> u64, words: usize) -> String {
    let mut out: Vec<String> =
        (0..words).map(|_| LOREM[(next() % LOREM.len() as u64) as usize].to_owned()).collect();
    if let Some(first) = out.first_mut() {
        let mut chars = first.chars();
        *first = chars
            .next()
            .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
            .unwrap_or_default();
    }
    out.join(" ") + "."
}

const FIRST: &[&str] = &[
    "Ana", "Maria", "Elena", "Ioana", "Andrei", "Mihai", "Alex", "John", "Emma", "Olivia", "Liam",
    "Noah", "Sophia", "Lucas", "Mia", "Ethan", "Chloe", "David", "Sara", "Daniel",
];
const LAST: &[&str] = &[
    "Popescu", "Ionescu", "Smith", "Johnson", "Brown", "Miller", "Davis", "Garcia", "Wilson",
    "Martin", "Taylor", "Moore", "Clark", "Lewis", "Walker", "Young", "King", "Wright", "Hill",
    "Green",
];
const CITIES: &[&str] = &[
    "Bucharest",
    "Cluj-Napoca",
    "Iasi",
    "Timisoara",
    "London",
    "Paris",
    "Berlin",
    "Madrid",
    "Rome",
    "Vienna",
    "Prague",
    "Lisbon",
    "Amsterdam",
    "Oslo",
    "Boston",
    "Seattle",
];
const COUNTRIES: &[&str] = &[
    "Romania",
    "United Kingdom",
    "Germany",
    "France",
    "Spain",
    "Italy",
    "Netherlands",
    "Sweden",
    "Japan",
    "Canada",
    "United States",
];
const JOBS: &[&str] = &[
    "Engineer",
    "Designer",
    "Developer",
    "Manager",
    "Analyst",
    "Architect",
    "Consultant",
    "Accountant",
    "Teacher",
    "Nurse",
];
const COLORS: &[&str] =
    &["red", "green", "blue", "orange", "purple", "teal", "maroon", "olive", "navy", "silver"];
const MATERIALS: &[&str] = &["Steel", "Wooden", "Cotton", "Granite", "Plastic", "Leather", "Glass"];
const PRODUCTS: &[&str] = &["Chair", "Table", "Lamp", "Shoes", "Hat", "Keyboard", "Bag", "Clock"];
const BUZZ: &[&str] = &[
    "Seamless",
    "Scalable",
    "Cloud-native",
    "Distributed",
    "Real-time",
    "Holistic",
    "Robust",
    "Intuitive",
];
const LOREM: &[&str] = &[
    "lorem",
    "ipsum",
    "dolor",
    "sit",
    "amet",
    "consectetur",
    "adipiscing",
    "elit",
    "sed",
    "do",
    "eiusmod",
    "tempor",
    "incididunt",
    "ut",
    "labore",
    "et",
    "dolore",
    "magna",
    "aliqua",
];

#[cfg(test)]
#[path = "../tests/unit/faker.rs"]
mod tests;
