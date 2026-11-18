#!/usr/bin/env bash
# Renders every screen (the editor and its parts, the tool windows, the modules' views, the
# modals and popovers, every settings page, the Windows top bar) in the dark and light
# themes, with the debug binary built here with the `snapshot` feature, to record a reference or compare against one
# pixel for pixel — the safety net of refactorings that must change nothing:
#
#   scripts/ui-snapshots.sh record  <dir> [group]   # the reference (before a change)
#   scripts/ui-snapshots.sh compare <dir> [group]   # after it: lists what differs, fails if any
#
# Groups: modals, editor, windows, modules, data, database, settings, chrome, themes (all when
# omitted), or one case: `case:<name>` (`case:agent-sessions`). `SNAPSHOT_LANG=en`: in
# English (the README's pictures; Romanian otherwise, as the references were made);
# `SNAPSHOT_SIZE=1600x1000`: the window that large (logical pixels; 1200x700 otherwise).
# The sample project, a sample git repository (fixed author and dates: the same hashes every
# run) and HOME live in a fixed scratch folder, so what's shown matches between runs;
# nothing of the user's (recent projects, settings, repositories) is read or written.
# Left out (not the same twice): the Task Manager measured (`TASKS=1`; its made-up
# scenarios are in), the remote wizard (needs
# the demo SFTP server), the local history (needs a versions folder).
set -euo pipefail

mode=${1:-}
out=${2:-}
only=${3:-}
if [[ "$mode" != record && "$mode" != compare ]] || [[ -z "$out" ]]; then
    echo "usage: $0 record|compare <dir> [modals|editor|windows|modules|data|database|settings|chrome|themes]" >&2
    exit 2
fi
root=$(cd "$(dirname "$0")/.." && pwd)
bin="$root/target/debug/$(sed -n 's/^id *= *"\(.*\)"/\1/p' "$root/brand.toml" | head -1)"
# The project's own folder (where a project keeps its data: `sql-files.toml`…).
project_dir="$(sed -n 's/^project_dir *= *"\(.*\)"/\1/p' "$root/brand.toml" | head -1)"
# The snapshots are only in a build with the `snapshot` feature (never in the builds handed
# out): built here, with it.
(cd "$root" && cargo build --quiet -p app --features snapshot) || { echo "the build failed" >&2; exit 2; }
[[ -x "$bin" ]] || { echo "no binary at $bin" >&2; exit 2; }

work="${TMPDIR:-/tmp}/ui-snapshots"
project="$work/project"
rm -rf "$work"
mkdir -p "$project/src" "$project/lib" "$work/home" "$out"

cat > "$project/src/Shop.php" <<'PHP'
<?php
namespace App;

use App\Lib\Cart;

/**
 * A shop: totals and carts.
 */
class Shop
{
    private Cart $cart;

    public function __construct(Cart $cart)
    {
        $this->cart = $cart;
    }

    public function total(int $a, int $b = 2): int
    {
        return $a + $b;
    }

    public function items(): array
    {
        return $this->cart->items();
    }
}
PHP
cat > "$project/src/Cart.php" <<'PHP'
<?php
namespace App\Lib;

class Cart
{
    public function items(): array
    {
        return [];
    }
}
PHP
cat > "$project/src/shapes.ts" <<'TS'
import { Logger } from './logger';

/** A shape with an area. */
export interface Shape {
  readonly name: string;
  area(): number;
}

export enum Color { Red = 'red', Green = 'green' }

export abstract class Base<T extends Shape> implements Shape {
  protected items: Array<T> = [];
  constructor(public readonly name: string, private log?: Logger) {}
  abstract area(): number;
}
TS
cat > "$project/src/Broken.php" <<'PHP'
<?php
function broken( {
    return 1
}
PHP
cat > "$project/style.css" <<'CSS'
.a {
    color: #3b82f6;
}
CSS
cat > "$project/README.md" <<'MD'
# Sample

Some **bold** text, a `code` span and a list:

- one
- two

```php
echo "hi";
```
MD
cat > "$project/GUIDE.md" <<'MD'
# Guide

## Lists

1. First, with *emphasis* and a [link](https://example.com)
2. Second
   - nested one
   - nested **two**
3. Third

> A quote, over
> two lines.

---

| Name | Kind | Size |
|------|:----:|-----:|
| Shop | class | 12 |
| total | method | 3 |

![logo](logo.png)

Text after the image, ~~struck~~ and `inline code`.
MD
printf 'one\ntwo\nthree\n' > "$project/left.txt"
printf 'one\n2\nthree\nfour\n' > "$project/right.txt"
# A small image (2×2 PNG).
printf '\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x02\x00\x00\x00\x02\x08\x02\x00\x00\x00\xfd\xd4\x9as\x00\x00\x00\x16IDATx\x9cc\xf8\xcf\xc0\xc0\xc0\xf0\x9f\x81\x81\x81\x01\x00\x0f\xfc\x03\xfd\xa8\x94\x1e\xd1\x00\x00\x00\x00IEND\xaeB`\x82' > "$project/logo.png"

# Files shown alone, outside the project (so its tree, its search and its repository stay
# as they are): SQL, a PHP heredoc holding SQL. The new languages' samples (C++, Bash, TOML,
# Dockerfile) are put in the project only for their own case (and removed after it), so the
# status bar shows their short path.
mkdir -p "$work/samples"
cat > "$work/samples/shop.sql" <<'SQL'
-- The shop's best customers this month.
CREATE TABLE IF NOT EXISTS orders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    total NUMERIC(10, 2) DEFAULT 0.0,
    paid BOOLEAN DEFAULT FALSE
);

/* Totals per customer, the biggest first. */
WITH recent AS (
    SELECT user_id, sum(total) AS spent
    FROM orders
    WHERE paid = TRUE AND created_at > date('now', '-30 days')
    GROUP BY user_id
)
SELECT u.email, r.spent,
       CASE WHEN r.spent > 1000 THEN 'gold' ELSE 'regular' END AS tier
FROM users u
JOIN recent r ON r.user_id = u.id
WHERE u.email LIKE ? AND u.deleted_at IS NULL
ORDER BY r.spent DESC
LIMIT 20;

UPDATE orders SET paid = TRUE WHERE id = :id;
SQL
cat > "$work/samples/Report.php" <<'PHP'
<?php

function bestCustomers(PDO $db, int $limit): array
{
    $sql = <<<SQL
        SELECT u.email, sum(o.total) AS spent
        FROM users u
        JOIN orders o ON o.user_id = u.id
        WHERE o.paid = TRUE -- paid only
        GROUP BY u.email
        ORDER BY spent DESC
        LIMIT ?
        SQL;
    $query = $db->prepare($sql);
    $query->execute([$limit]);
    return $query->fetchAll();
}
PHP

cat > "$work/samples/shapes.cpp" <<'CPP'
// Shapes and their total area.
#include <memory>
#include <vector>

namespace geometry {

constexpr double PI = 3.14159265358979;

class Shape {
public:
    virtual ~Shape() = default;
    virtual double area() const noexcept = 0;
};

class Circle final : public Shape {
public:
    explicit Circle(double radius) : radius_(radius) {}
    double area() const noexcept override { return PI * radius_ * radius_; }

private:
    double radius_;
};

template <typename T>
T total(const std::vector<std::unique_ptr<Shape>> &shapes) {
    T sum{};
    for (const auto &shape : shapes) {
        sum += static_cast<T>(shape->area());
    }
    return sum;
}

}  // namespace geometry

int main() {
    std::vector<std::unique_ptr<geometry::Shape>> shapes;
    shapes.push_back(std::make_unique<geometry::Circle>(2.0));
    return geometry::total<double>(shapes) > 10.0 ? 0 : 1;
}
CPP

cat > "$work/samples/deploy.sh" <<'SH'
#!/usr/bin/env bash
# Deploys the shop: builds the assets, then syncs them to the server.
set -euo pipefail

readonly HOST="${DEPLOY_HOST:-shop.example.com}"
release=$(date +%Y%m%d%H%M)

build() {
    local mode="$1"
    npm run "build:$mode" --silent || return 1
    echo "built ($mode)"
}

if [[ ! -d public/build ]]; then
    build production
fi

for dir in public/build vendor; do
    rsync -az --delete "$dir/" "deploy@$HOST:/srv/shop/$release/$dir/"
done

case "${1:-}" in
    --dry-run) echo "nothing changed" ;;
    *) ssh "deploy@$HOST" "ln -sfn /srv/shop/$release /srv/shop/current" ;;
esac
SH

cat > "$work/samples/Cargo.toml" <<'TOML'
# The shop's command-line tools.
[package]
name = "shop-cli"
version = "0.3.1"
edition = "2024"
authors = ["Ana <ana@example.com>"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
clap = "4.5"
tokio.workspace = true

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[[bin]]
name = "shop"
path = "src/main.rs"
test = false

[profile.release]
opt-level = 3
lto = true
TOML

cat > "$work/samples/Dockerfile" <<'DOCKER'
# syntax=docker/dockerfile:1
ARG PHP_VERSION=8.4

# The assets, built once.
FROM node:22-alpine AS assets
WORKDIR /app
COPY package.json package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci \
    && npm run build

FROM php:${PHP_VERSION}-fpm AS app
ENV APP_ENV=production \
    APP_DEBUG=0
COPY --from=assets /app/public/build /var/www/public/build
RUN <<EOT
apt-get update
apt-get install -y --no-install-recommends libzip-dev
EOT
EXPOSE 9000
USER www-data
HEALTHCHECK --interval=30s CMD php-fpm -t || exit 1
CMD ["php-fpm", "--nodaemonize"]
DOCKER

# A family of types in one file (outside the project: its tree stays as it is), for the
# navigation's lists (plan 43j).
cat > "$work/samples/Shapes.php" <<'PHP'
<?php
namespace App\Shapes;

interface Shape
{
    public function area(): float;
}

abstract class Polygon implements Shape
{
    abstract public function sides(): int;
}

class Square extends Polygon
{
    public function area(): float { return 4.0; }
    public function sides(): int { return 4; }
}

class Circle implements Shape
{
    public function area(): float { return 3.14; }
}
PHP

# A class the project declares (App\Lib\Cart), written without its import: the bulb and
# the context actions (plan 43j).
cat > "$work/samples/Order.php" <<'PHP'
<?php
namespace App;

class Order
{
    public function cart(): array
    {
        return [new Cart()];
    }
}
PHP

# The sample repository: two commits, fixed author and dates.
(
    cd "$project"
    export GIT_AUTHOR_NAME=Sample GIT_AUTHOR_EMAIL=sample@example.com
    export GIT_COMMITTER_NAME=Sample GIT_COMMITTER_EMAIL=sample@example.com
    export GIT_AUTHOR_DATE="2026-01-01T10:00:00Z" GIT_COMMITTER_DATE="2026-01-01T10:00:00Z"
    export HOME="$work/home" GIT_CONFIG_NOSYSTEM=1
    git init -q -b main
    git add -A
    git commit -q -m "First version"
    export GIT_AUTHOR_DATE="2026-01-02T10:00:00Z" GIT_COMMITTER_DATE="2026-01-02T10:00:00Z"
    sed -i.bak 's/return \$a + \$b;/return $a + $b + 0;/' src/Shop.php && rm src/Shop.php.bak
    git commit -q -am "Totals: explicit"
    # A change not committed (the gutter, "compared with the last commit").
    sed -i.bak 's/A shop: totals and carts./A shop: totals, carts./' src/Shop.php && rm src/Shop.php.bak
)

# A file's history with a long message and many files (Git's history tab, its details
# scrolling, its menus): a repository of its own, beside the project (fixed author and dates).
history="$work/history"
mkdir -p "$history/src"
(
    cd "$history"
    export GIT_AUTHOR_NAME=Sample GIT_AUTHOR_EMAIL=sample@example.com
    export GIT_COMMITTER_NAME=Sample GIT_COMMITTER_EMAIL=sample@example.com
    export GIT_AUTHOR_DATE="2026-01-01T10:00:00Z" GIT_COMMITTER_DATE="2026-01-01T10:00:00Z"
    export HOME="$work/home" GIT_CONFIG_NOSYSTEM=1
    printf '{\n    "indexer.exclude_patterns": []\n}\n' > .phpactor.json
    for i in $(seq -w 1 54); do printf '<?php\n// Part %s\n' "$i" > "src/Part$i.php"; done
    git init -q -b main
    git add -A
    git commit -q -m "Sample: the parts"
    export GIT_AUTHOR_DATE="2026-01-03T10:00:00Z" GIT_COMMITTER_DATE="2026-01-03T10:00:00Z"
    printf '{\n    "indexer.exclude_patterns": ["/vendor/**"]\n}\n' > .phpactor.json
    for i in $(seq -w 1 54); do printf 'echo %s;\n' "$i" >> "src/Part$i.php"; done
    {
        echo "Index: leave the vendor folder out and echo every part"
        echo
        for i in $(seq 1 18); do
            echo "Paragraph $i of the explanation: why the parts changed together, what the"
            echo "indexer now skips, and how the language server reads the project after it."
            echo
        done
        echo "Refs: SAMPLE-42"
    } > "$work/message.txt"
    git commit -q -a -F "$work/message.txt"
)

# group|name|assignments (`;`-separated, `EDITOR_SNAPSHOT_` added unless given)|file to open
cases=(
    "modals|about|ABOUT=1|src/Shop.php"
    "modals|dialog|DIALOG=1|src/Shop.php"
    "modals|dialog-rename|DIALOG=rename|src/Shop.php"
    "modals|projects|PROJECTS=1|src/Shop.php"
    "modals|projects-groups|PROJECTS=groups|src/Shop.php"
    "modals|projects-menu|PROJECTS=menu|src/Shop.php"
    "modals|projects-drag|PROJECTS=drag|src/Shop.php"
    "modals|dialog-delete|DIALOG=delete|src/Shop.php"
    "modals|dialog-delete-many|DIALOG=delete-many|src/Shop.php"
    "modals|running|DIALOG=running|src/Shop.php"
    "modals|running-wait|DIALOG=running-wait|src/Shop.php"
    "modals|search|SEARCH=total|src/Shop.php"
    "modals|search-place|SEARCH=Shop.php:7:9|src/Shop.php"
    "modals|level|LEVEL=1|src/Shop.php"
    "modals|color|COLOR=1|style.css"
    "modals|menu|MENU=1|src/Shop.php"
    "modals|copy|COPY=1|src/Shop.php"
    "modals|settings-query|SETTINGS=0;EDITOR_SNAPSHOT_SETTINGS_QUERY=tema|src/Shop.php"
    "editor|editor||src/Shop.php"
    "editor|editor-css||style.css"
    "editor|editor-error||src/Broken.php"
    "editor|editor-fold|FOLD=1|src/Shop.php"
    # A TypeScript file answered by the fake language server: its diagnostics and its
    # completion (43a).
    "editor|lsp-completion|LSP=completion|src/Shop.php"
    # A CSS file answered by it as the CSS server is described (43b): its completion and
    # warning, the built-in engine's deprecation kept.
    "editor|lsp-css-completion|LSP=css|src/Shop.php"
    "editor|editor-find|FIND=total|src/Shop.php"
    "editor|editor-select|SELECT=total|src/Shop.php"
    "editor|editor-type|TYPE=\$this->c;LINE=15|src/Shop.php"
    # Our PHP engine's inference (plan 47): a chained call's completion (a built-in class's
    # method returning static), a variable's hover (its type, where it comes from).
    "editor|php-inference-completion|TYPE=\\n\$d = new \\DateTimeImmutable(){sc}\\n\$d->modify('+1 day')->;LINE=24|src/Shop.php"
    "editor|php-variable-hover|INSERT=\\n        \$made = new Shop(\$this->cart){sc} \$made->total(1){sc};LINE=24;HOVER_WORD=\$made|src/Shop.php"
    "editor|editor-zoom|ZOOM=1.5|src/Shop.php"
    "editor|editor-pin|PIN=0|src/Shop.php"
    "editor|editor-split|SPLIT=right,down|src/Shop.php"
    "editor|editor-link|LINK=600,560|src/Shop.php"
    "windows|tree-files|FILES=sho|src/Shop.php"
    "windows|tree-new|NEW=Foo.php|src/Shop.php"
    "windows|terminal|TERMINAL=echo hello|src/Shop.php"
    # Output longer than the screen: the history's scrollbar in its lane, at the bottom.
    "windows|terminal-scrollback|TERMINAL=seq 1 300|src/Shop.php"
    "windows|structure|PANEL=structure|src/Shop.php"
    # A C++ file's Structure from the fake language server's declarations (46d).
    "windows|structure-cpp-server|LSP=cpp;PANEL=structure|src/Shop.php"
    # The Services window at the bottom: nothing contributed, then a sample contributor.
    "windows|services-empty|SERVICES=empty|src/Shop.php"
    "windows|services-fake|SERVICES=fake|src/Shop.php"
    # The project's agent sessions (made up: Claude Code's files written in the scratch
    # HOME, nothing of the user's read), the README's picture.
    "modules|agent-sessions|PANEL=agent-sessions;SESSIONS_SAMPLE=1|src/Shop.php"
    "modules|canvas-1|CANVAS=1|src/Shop.php"
    "modules|canvas-2|CANVAS=2|src/Shop.php"
    "modules|canvas-3|CANVAS=3|src/Shop.php"
    "modules|compare|COMPARE=$project/left.txt,$project/right.txt|left.txt"
    "modules|compare-merge|COMPARE=$project/left.txt,$project/right.txt,merge|left.txt"
    "modules|markdown||README.md"
    "modules|markdown-full||GUIDE.md"
    "modules|image||logo.png"
    "modules|studio|STUDIO=1|src/Shop.php"
    "modules|studio-caret|STUDIO=1;STUDIO_COMMAND=scroll-to:caret|src/Shop.php"
    # The studio's decorations on a theme without any, from the Autumn preset (plan 41f).
    "modules|studio-layers|STUDIO=1;STUDIO_COMMAND=preset:autumn,scroll-to:decorations|src/Shop.php"
    # The rethought studio (plan 42b): each category, a search, narrow (in a split).
    "modules|studio-interface|STUDIO=1;STUDIO_COMMAND=category:interface|src/Shop.php"
    "modules|studio-syntax|STUDIO=1;STUDIO_COMMAND=category:syntax|src/Shop.php"
    "modules|studio-search|STUDIO=1;STUDIO_COMMAND=search:fundal|src/Shop.php"
    "modules|studio-narrow|STUDIO=1;SPLIT=right;STUDIO_COMMAND=category:caret|src/Shop.php"
    # Its effects (plan 42c): an effect's card opened, the gallery.
    "modules|studio-effect-open|STUDIO=1;STUDIO_COMMAND=preset:winter,category:effects,open-all|src/Shop.php"
    "modules|studio-gallery|STUDIO=1;STUDIO_COMMAND=category:effects,gallery|src/Shop.php"
    # The gallery scrolled to its end: the effects by place, plan 45's among them.
    "modules|studio-gallery-effects|STUDIO=1;STUDIO_COMMAND=category:effects,gallery:end|src/Shop.php"
    # A short window (48l): the side navigation and the content both scroll, each with its
    # scrollbar; narrow and short, the tabs over a scrolling content.
    "settings|settings-short|SIZE=1200x440;SETTINGS=0|src/Shop.php"
    "settings|settings-short-narrow|SIZE=700x460;SETTINGS=0|src/Shop.php"
    "settings|settings-caret|SETTINGS=0;SETTINGS_QUERY=halou;SETTINGS_OPEN=1|src/Shop.php"
    "settings|settings-notifications|SETTINGS=0;SETTINGS_QUERY=notificări;SETTINGS_OPEN=1|src/Shop.php"
    # The language servers: each language's engine, "Add a server" (43a).
    "settings|settings-language-servers|SETTINGS=0;SETTINGS_QUERY=servere de limbaj;SETTINGS_OPEN=1|src/Shop.php"
    "settings|settings-language-servers-add|SETTINGS=0;SETTINGS_QUERY=adaugă un server;SETTINGS_OPEN=1|src/Shop.php"
    # PHP's engines (43h): intelephense not included (Install), installed (its licence key's
    # field, a licence file offered), with a key kept.
    "settings|settings-language-servers-php|LSP_PHP=missing;SETTINGS=0;SETTINGS_QUERY=servere de limbaj;SETTINGS_OPEN=1|src/Shop.php"
    "settings|settings-language-servers-php-installed|LSP_PHP=installed;SETTINGS=0;SETTINGS_QUERY=servere de limbaj;SETTINGS_OPEN=1|src/Shop.php"
    "settings|settings-language-servers-php-licensed|LSP_PHP=licensed;SETTINGS=0;SETTINGS_QUERY=servere de limbaj;SETTINGS_OPEN=1|src/Shop.php"
    # The servers downloaded on demand (43i): the bar over a file whose server isn't there,
    # the same while it downloads, the settings' list of what's downloaded.
    "editor|lsp-offer|LSP_OFFER=offer|src/shapes.ts"
    "editor|lsp-offer-downloading|LSP_OFFER=downloading|src/shapes.ts"
    # PHP's servers to pick between (plan 43j): intelephense (installed, its licence agreed
    # by the click) first, phpactor (downloaded); the engine switcher once one is there.
    "editor|lsp-offer-php|LSP_OFFER=offer|src/Shop.php"
    "editor|lsp-engine-switch|LSP_PHP=installed;ENGINES=1|src/Shop.php"
    "settings|settings-language-servers-parts|LSP_OFFER=parts;SETTINGS=0;SETTINGS_QUERY=servere descărcate;SETTINGS_OPEN=1|src/Shop.php"
    # A theme's own settings in Appearance, while it's in use (plan 41e).
    "settings|settings-theme-options|THEMED=christmas;SETTINGS=0;SETTINGS_QUERY=opțiuni;SETTINGS_OPEN=1|src/Shop.php"
    "settings|settings-shortcut|SETTINGS=7;SHORTCUT=window.new;SHORTCUT_KEYS=CmdOrCtrl+S|src/Shop.php"
    # The reorganized settings (plan 44): the new pages, the Languages page with PHP and with
    # SQL, the language servers' "Advanced" open, a narrow window (tabs).
    "settings|settings-page-files|SETTINGS=0;SETTINGS_PAGE=files|src/Shop.php"
    "settings|settings-page-languages-php|SETTINGS=0;SETTINGS_PAGE=languages;SETTINGS_LANGUAGE=php|src/Shop.php"
    "settings|settings-page-languages-sql|SETTINGS=0;SETTINGS_PAGE=languages;SETTINGS_LANGUAGE=sql|src/Shop.php"
    # The project's own ESLint and Prettier (its node_modules) offered for JavaScript (46c).
    "settings|settings-page-languages-javascript|SETTINGS=0;SETTINGS_PAGE=languages;SETTINGS_LANGUAGE=javascript;NODE_TOOLS=1|src/Shop.php"
    "settings|settings-page-language-servers|LSP_OFFER=parts;SETTINGS=0;SETTINGS_PAGE=language-servers;SETTINGS_ADVANCED=1|src/Shop.php"
    "settings|settings-page-database|SETTINGS=0;SETTINGS_PAGE=database;SETTINGS_ADVANCED=1|src/Shop.php"
    "settings|settings-page-tasks|SETTINGS=0;SETTINGS_PAGE=tasks|src/Shop.php"
    "settings|settings-page-notifications|SETTINGS=0;SETTINGS_PAGE=notifications|src/Shop.php"
    "settings|settings-narrow|SIZE=760x620;SETTINGS=0;SETTINGS_PAGE=languages|src/Shop.php"
    "modules|git-head|GIT=head|src/Shop.php"
    "modules|git-history|GIT=history|src/Shop.php"
    "modules|git-log|GIT=log|src/Shop.php"
    "modules|git-changes|GIT=changes|src/Shop.php"
    "modules|git-blame|GIT=blame|src/Shop.php"
    "modules|git-annotated|GIT=annotated|src/Shop.php"
    "modules|git-diff|GIT=diff|src/Shop.php"
    # A file's history: a long message and 54 files, scrolled to the files; its menus.
    "modules|git-file-history-scrolled|GIT=file-history-scrolled|history:.phpactor.json"
    "modules|git-file-history-commit-menu|GIT=file-history-commit-menu|history:.phpactor.json"
    "modules|git-file-history-file-menu|GIT=file-history-file-menu|history:.phpactor.json"
    "modules|rename|SELECT=function total;RENAME=sum|src/Shop.php"
    "modules|usages|SELECT=items;USAGES=1|src/Shop.php"
    # Find Usages from the fake language server's references joined with the built-in
    # engine's in a page (46a).
    "modules|lsp-usages|LSP=usages;SELECT=function cartTotal;USAGES=1|src/Shop.php"
    "modules|diagram|SELECT=Shop;DIAGRAM=1|src/Shop.php"
    "modules|signature|SELECT=function total;SIGNATURE=1|src/Shop.php"
    "modules|move|MOVE=lib|src/Shop.php"
    "modules|clipboard|CLIPBOARD=one\\ntwo|src/Shop.php"
    "editor|editor-typescript||src/shapes.ts"
    "editor|sql-editor||../samples/shop.sql"
    "editor|php-heredoc-sql||../samples/Report.php"
    # What implements an interface, a type's hierarchy (plan 43j).
    "editor|navigation-implementations|CURSOR=interface Sha;RELATED=implementations|../samples/Shapes.php"
    "editor|navigation-hierarchy|CURSOR=class Poly;RELATED=hierarchy|../samples/Shapes.php"
    "editor|context-actions|CURSOR=new Car;RELATED=actions|../samples/Order.php"
    # A call with literal arguments in Shop.php: the parameters' names (inlay hints) and
    # the lenses after the declarations (plan 43j).
    "editor|insight|INSERT=\\n        \$this->total(1, 3){sc};LINE=24|src/Shop.php"
    "editor|editor-cpp||src/shapes.cpp"
    # The languages of 43d, each from its sample put in the project for its case.
    "editor|editor-bash||deploy.sh"
    "editor|editor-toml||Cargo.toml"
    "editor|editor-dockerfile||Dockerfile"
    # The data view: sample files made by the snapshot itself, outside the project.
    "data|data-sqlite|DATA=sqlite|src/Shop.php"
    "data|data-structure|DATA=structure|src/Shop.php"
    "data|data-query|DATA=query|src/Shop.php"
    "data|data-transpose|DATA=transpose|src/Shop.php"
    "data|data-find|DATA=find|src/Shop.php"
    "data|data-menu|DATA=menu|src/Shop.php"
    "data|data-csv|DATA=csv|src/Shop.php"
    "data|data-csv-split|DATA=split|src/Shop.php"
    "data|data-edit|DATA=edit|src/Shop.php"
    "data|data-preview|DATA=preview|src/Shop.php"
    "data|data-csv-split-keys|DATA=split-keys|src/Shop.php"
    "data|data-json|DATA=json|src/Shop.php"
    "data|data-where-error|DATA=where-error|src/Shop.php"
    # The grid's data extractors (plan 39o): the "CSV ▾" popover open.
    "data|grid-extractors|DATA=extractors|src/Shop.php"
    "data|data-export|DATA=export|src/Shop.php"
    "data|data-exported|DATA=exported|src/Shop.php"
    "data|data-create-table|DATA=create-table|src/Shop.php"
    "data|data-alter-column|DATA=alter-column|src/Shop.php"
    "data|data-drop-column|DATA=drop-column|src/Shop.php"
    "data|data-create-index|DATA=create-index|src/Shop.php"
    # The "Database" window: the sample files as sources (in memory only).
    "database|database-tree|DATABASE=tree|src/Shop.php"
    # A server's databases under its source (its own, empty, marked), one opened.
    "database|database-tree-databases|DATABASE=databases|src/Shop.php"
    # A MySQL server's databases as its schemas, one session for all, one opened.
    "database|database-tree-mysql|DATABASE=mysql|src/Shop.php"
    "database|database-add|DATABASE=add|src/Shop.php"
    "database|database-menu|DATABASE=menu|src/Shop.php"
    "database|database-sources|DATABASE=sources|src/Shop.php"
    "database|database-sources-ssl|DATABASE=sources-ssl|src/Shop.php"
    "database|database-sources-schemas|DATABASE=sources-schemas|src/Shop.php"
    "database|database-sources-ok|DATABASE=sources-ok|src/Shop.php"
    "database|database-sources-failed|DATABASE=sources-failed|src/Shop.php"
    "database|database-sources-folders|DATABASE=sources-folders|src/Shop.php"
    "database|database-folders|DATABASE=folders|src/Shop.php"
    "database|database-folder-grid|DATABASE=folder-grid|src/Shop.php"
    "database|database-folder-grid-more|DATABASE=folder-grid-more|src/Shop.php"
    "database|database-folder-grid-tip|DATABASE=folder-grid-tip|src/Shop.php"
    # A query console of the sample database (its clock stopped), and its source's consoles.
    "database|console|CONSOLE=run|src/Shop.php"
    "database|database-jump-console|CONSOLE=jump|src/Shop.php"
    # The consoles' popover filtered (names and texts matched), and a console's history.
    "database|database-jump-console-filter|CONSOLE=jump-filter|src/Shop.php"
    "database|console-history|CONSOLE=history|src/Shop.php"
    "database|console-history-clear|CONSOLE=history-clear|src/Shop.php"
    # Completion from the source's schema: the tables with their source after FROM, the
    # related tables with their ON condition after JOIN.
    "database|console-completion|CONSOLE=completion|src/Shop.php"
    # A function's parameter help in a console (48c).
    "database|console-signature|CONSOLE=signature|src/Shop.php"
    "database|console-join|CONSOLE=join|src/Shop.php"
    # Names resolved through the schema: a table's hover card (its columns, types, keys);
    # an unknown table underlined with its message (a CTE beside it isn't).
    "database|console-hover|CONSOLE=hover|src/Shop.php"
    "database|console-unknown-table|CONSOLE=unknown-table|src/Shop.php"
    # A SQL file of the project (39n): "No data source — Attach…", then attached to the
    # source with a statement run (the source beside the schema, its result in Services).
    "database|sql-file|CONSOLE=file|src/Shop.php"
    "database|sql-file-attached|CONSOLE=file-attached|src/Shop.php"
    # Its results in the Services window: the Output, a table's rows, a script's results (the
    # first pinned), an error underlined in the console and red in the Output.
    "database|services-output|CONSOLE=output|src/Shop.php"
    "database|services-result|CONSOLE=result|src/Shop.php"
    # A result of one table's rows edited in place: a cell changed, a row added, one marked
    # for deletion, Submit waiting (plan 39l).
    "database|services-result-edit|CONSOLE=result-edit|src/Shop.php"
    # A result's grid with its data extractors' popover open (plan 39o).
    "database|services-result-extractors|CONSOLE=result-extractors|src/Shop.php"
    "database|services-results-many|CONSOLE=results-many|src/Shop.php"
    # A data extractor's template (39o2): its folder made with the examples, one open, colored.
    "database|extractor-template|EXTRACTOR=template|src/Shop.php"
    # The Task Manager (plan 40e), made up — fixed parts, programs, operations and history,
    # its clock stopped: the whole tab (Wide, a terminal selected, its detail on the right),
    # in a split dragged to ~600 / ~400 / ~280 px (Narrow, Compact as a list, Minimal), its
    # detail as a page in a narrow split, the columns' chooser, every project's view.
    "modules|task-manager|TASKS=wide|src/Shop.php"
    "modules|task-manager-split|SPLIT=right;SPLIT_AT=556;TASKS=split|src/Shop.php"
    "modules|task-manager-compact|SPLIT=right;SPLIT_AT=756;TASKS=compact|src/Shop.php"
    "modules|task-manager-minimal|SPLIT=right;SPLIT_AT=876;TASKS=minimal|src/Shop.php"
    "modules|task-manager-page|SPLIT=right;SPLIT_AT=700;TASKS=page|src/Shop.php"
    "modules|task-manager-columns|TASKS=columns|src/Shop.php"
    "modules|task-manager-all|TASKS=all|src/Shop.php"
    # The project's language servers (plan 43a, made up): vtsls selected, its figures shown.
    "modules|task-manager-servers|TASKS=servers|src/Shop.php"
    # A row's right-click menu (plan 40f): show in the detail, copy the row, its programs' ids.
    "modules|task-manager-menu|TASKS=menu|src/Shop.php"
    # Stopping a program asked first (plan 40g): the dialog over the tab, a formatter selected.
    "modules|task-manager-stop|TASKS=stop|src/Shop.php"
    "database|console-error|CONSOLE=error|src/Shop.php"
    # Transactions and safety: two writes waiting for Commit in Tx: Manual; a DELETE without
    # WHERE and a DROP confirmed first; a production write in Tx: Auto ("I understand").
    "database|console-tx-manual|CONSOLE=tx-manual|src/Shop.php"
    "database|console-danger|CONSOLE=danger|src/Shop.php"
    "database|console-prod-confirm|CONSOLE=prod-confirm|src/Shop.php"
    # A console's right-click menu: its own rows first (Execute, Explain Plan, Jump, Open in
    # Services), Comment, the clipboard; none of PHP's refactorings.
    "database|console-menu|CONSOLE=menu|src/Shop.php"
    # Parameters asked before a run (written in by the dialect, kept in memory only), and a
    # plan explained as a tree on columns in the Services window.
    "database|console-params|CONSOLE=params|src/Shop.php"
    # A statement's profile: a PostgreSQL one made by hand (parallel, a spill, problems) in
    # the Services window wide and narrow, in an editor tab of its own, a finding chosen
    # (its place lit in the console), as a diagram and as a flame graph; and Explain Analyze
    # measured on the sample SQLite.
    "database|services-profile|CONSOLE=profile|src/Shop.php"
    "database|services-profile-narrow|CONSOLE=profile-narrow|src/Shop.php"
    "database|profile-tab|CONSOLE=profile-tab|src/Shop.php"
    "database|profile-finding-selected|CONSOLE=profile-finding|src/Shop.php"
    "database|services-profile-graph|CONSOLE=profile-graph|src/Shop.php"
    "database|services-profile-flame|CONSOLE=profile-flame|src/Shop.php"
    # Two profiles compared (before and after an index) and a profile's tables with the
    # server's statistics of them (both made by hand).
    "database|services-profile-compare|CONSOLE=profile-compare|src/Shop.php"
    "database|services-profile-tables|CONSOLE=profile-tables|src/Shop.php"
    "database|profile-sqlite|CONSOLE=profile-sqlite|src/Shop.php"
    # The relations diagram of a sample store (made by the snapshot itself).
    "database|relations|RELATIONS=all|src/Shop.php"
    "database|relations-focus|RELATIONS=focus|src/Shop.php"
    "database|relations-inferred|RELATIONS=inferred|src/Shop.php"
    "database|relations-decided|RELATIONS=decided|src/Shop.php"
    "database|relations-edge|RELATIONS=edge|src/Shop.php"
    # The notifications: every kind (their time stopped part way), at each edge, over a modal.
    "chrome|toasts|TOASTS=all|src/Shop.php"
    "chrome|toasts-top-right|TOASTS=all;TOAST_POSITION=top-right|src/Shop.php"
    "chrome|toasts-bottom-right|TOASTS=all;TOAST_POSITION=bottom-right|src/Shop.php"
    "chrome|toasts-bottom-center|TOASTS=all;TOAST_POSITION=bottom-center|src/Shop.php"
    "database|toast-over-modal|DATABASE=sources;TOASTS=modal|src/Shop.php"
    "chrome|window-controls|WINDOW_CONTROLS=1|src/Shop.php"
    "chrome|window-max|WINDOW_CONTROLS=max|src/Shop.php"
    "chrome|window-full|WINDOW_CONTROLS=full-shown|src/Shop.php"
    "chrome|splash|SPLASH=1|src/Shop.php"
    # The menu drawer (plan 49): the button in the window's bar (as on Windows), the drawer
    # open, with File's submenu (a row chosen), filtered, in a short window (the categories
    # and View's rows scrolling), over Matrix's rain (the effects stay behind it).
    "chrome|menu-button|MENU_DRAWER=closed;WINDOW_CONTROLS=1|src/Shop.php"
    "chrome|menu-drawer|MENU_DRAWER=open|src/Shop.php"
    "chrome|menu-drawer-submenu|MENU_DRAWER=keys:ddrdd|src/Shop.php"
    "chrome|menu-drawer-filter|MENU_DRAWER=filter:sal|src/Shop.php"
    "chrome|menu-drawer-short|MENU_DRAWER=keys:ddddr;SIZE=1200x440|src/Shop.php"
    "themes|menu-drawer-matrix|MENU_DRAWER=keys:ddd;THEME=matrix|src/Shop.php"
    # Each menu of the bar in the drawer (plan 51, macOS' order: the application menu
    # first), a row's own submenu as the third panel (View › Tool Windows; Code › Folding,
    # its row chosen with the keys), in a window too narrow for three panels (over the
    # second, with its header back), the filter finding a submenu's rows by its title
    # ("plier": Cod › Pliere ›, the snapshots being in Romanian).
    "chrome|menu-drawer-app|MENU_DRAWER=keys:d|src/Shop.php"
    "chrome|menu-drawer-file|MENU_DRAWER=keys:dd|src/Shop.php"
    "chrome|menu-drawer-edit|MENU_DRAWER=keys:ddd|src/Shop.php"
    "chrome|menu-drawer-view|MENU_DRAWER=keys:dddd|src/Shop.php"
    "chrome|menu-drawer-find|MENU_DRAWER=keys:ddddd|src/Shop.php"
    "chrome|menu-drawer-navigate|MENU_DRAWER=keys:dddddd|src/Shop.php"
    "chrome|menu-drawer-code|MENU_DRAWER=keys:ddddddd|src/Shop.php"
    "chrome|menu-drawer-refactor|MENU_DRAWER=keys:dddddddd|src/Shop.php"
    "chrome|menu-drawer-git|MENU_DRAWER=keys:ddddddddd|src/Shop.php"
    "chrome|menu-drawer-tools|MENU_DRAWER=keys:dddddddddd|src/Shop.php"
    "chrome|menu-drawer-window|MENU_DRAWER=keys:ddddddddddd|src/Shop.php"
    "chrome|menu-drawer-help|MENU_DRAWER=keys:dddddddddddd|src/Shop.php"
    "chrome|menu-drawer-third|MENU_DRAWER=keys:ddddrr|src/Shop.php"
    "chrome|menu-drawer-third-folding|MENU_DRAWER=keys:dddddddrdddddr|src/Shop.php"
    "chrome|menu-drawer-third-narrow|MENU_DRAWER=keys:ddddrr;SIZE=900x600|src/Shop.php"
    "chrome|menu-drawer-filter-fold|MENU_DRAWER=filter:plier|src/Shop.php"
    # What doesn't apply is greyed out (plan 51d): Git's menu with nothing open (every row,
    # its reason on the chosen one) and in the sample's repository with a file (every row).
    "chrome|menu-drawer-git-none|MENU_DRAWER=keys:ddddddddd|-"
    "chrome|menu-drawer-git-reason|MENU_DRAWER=keys:dddddddddr|-"
    "chrome|menu-drawer-git-repo|MENU_DRAWER=keys:dddddddddr|src/Shop.php"
    # The backdrop under the windows over the editor (plan 50): Glass (gently blurred, the
    # default 4 px) under the settings, a dialog and the menu drawer; Matrix asks for it
    # itself; the Appearance page scrolled to its end (the backdrop's block, the menu's
    # last); the studio's section with its preview's dialog on Glass.
    "modals|glass-settings|SETTINGS=0;BACKDROP=glass|src/Shop.php"
    "modals|glass-dialog|DIALOG=1;BACKDROP=glass|src/Shop.php"
    "chrome|glass-drawer|MENU_DRAWER=open;BACKDROP=glass|src/Shop.php"
    "themes|glass-matrix|SETTINGS=0;THEME=matrix|src/Shop.php"
    "settings|settings-backdrop|SETTINGS=0;SETTINGS_SCROLL=20000|src/Shop.php"
    "modules|studio-backdrop|STUDIO=1;STUDIO_COMMAND=category:interface,scroll-to:backdrop,backdrop:glass|src/Shop.php"
    # The Christmas theme (plan 41): its garland, corners and snow as a still frame — its
    # dark variant in the dark run, its light one in the light run; and its light variant
    # in both.
    "themes|theme-christmas|THEMED=christmas|src/Shop.php"
    "themes|theme-christmas-light|THEME=christmas-light|src/Shop.php"
    # Matrix with its digital rain on by default, and the Cyberpunk theme with its neon
    # (plan 45), still frames.
    "themes|theme-matrix|THEME=matrix|src/Shop.php"
    "themes|theme-cyberpunk|THEME=cyberpunk|src/Shop.php"
    # The two themes whose muted text was faintest before 48e (now 4.5:1 on a card).
    "themes|theme-solar-light|THEME=solar-light|src/Shop.php"
    "themes|theme-latte|THEME=latte|src/Shop.php"
    # Decorations for any theme (plan 41d): presets drawn over the run's theme, still.
    "themes|decorations-autumn|MY_DECORATIONS=autumn|src/Shop.php"
    "themes|decorations-new-year|MY_DECORATIONS=new-year|src/Shop.php"
    "themes|decorations-winter|MY_DECORATIONS=winter|src/Shop.php"
    # Plan 42's effects, each a still frame (behind the panels, where they show through).
    "themes|effect-matrix|MY_DECORATIONS=matrix@panels|src/Shop.php"
    "themes|effect-starfield|MY_DECORATIONS=starfield@panels|src/Shop.php"
    "themes|effect-aurora|MY_DECORATIONS=aurora@panels|src/Shop.php"
    "themes|effect-fireflies|MY_DECORATIONS=fireflies|src/Shop.php"
    "themes|effect-bubbles|MY_DECORATIONS=bubbles|src/Shop.php"
    "themes|effect-fireworks|MY_DECORATIONS=fireworks@everywhere|src/Shop.php"
    "themes|effect-shooting-stars|MY_DECORATIONS=shooting-stars@edges|src/Shop.php"
    "themes|effect-waves|MY_DECORATIONS=waves@panels|src/Shop.php"
    "themes|effect-neon|MY_DECORATIONS=neon|src/Shop.php"
    "themes|effect-constellation|MY_DECORATIONS=constellation@panels|src/Shop.php"
    "themes|effect-synthwave|MY_DECORATIONS=synthwave@panels|src/Shop.php"
    # The presets of plan 42.
    "themes|decorations-cyberpunk|MY_DECORATIONS=cyberpunk|src/Shop.php"
    "themes|decorations-space|MY_DECORATIONS=space|src/Shop.php"
    "themes|decorations-ocean|MY_DECORATIONS=ocean|src/Shop.php"
    "themes|decorations-synthwave|MY_DECORATIONS=synthwave|src/Shop.php"
    # The effects and presets of plan 42h–42i.
    "themes|effect-lightning|MY_DECORATIONS=lightning@panels|src/Shop.php"
    "themes|effect-raindrops|MY_DECORATIONS=raindrops@panels|src/Shop.php"
    "themes|effect-clouds|MY_DECORATIONS=clouds|src/Shop.php"
    "themes|effect-fog|MY_DECORATIONS=fog@panels|src/Shop.php"
    "themes|effect-butterflies|MY_DECORATIONS=butterflies@panels|src/Shop.php"
    "themes|decorations-storm|MY_DECORATIONS=storm|src/Shop.php"
    "themes|effect-balloons|MY_DECORATIONS=balloons@panels|src/Shop.php"
    "themes|effect-hearts|MY_DECORATIONS=hearts@panels|src/Shop.php"
    "themes|effect-bats|MY_DECORATIONS=bats@panels|src/Shop.php"
    "themes|effect-lava-lamp|MY_DECORATIONS=lava-lamp@panels|src/Shop.php"
    "themes|effect-crt|MY_DECORATIONS=crt@panels|src/Shop.php"
    "themes|decorations-valentine|MY_DECORATIONS=valentine|src/Shop.php"
    "themes|decorations-party|MY_DECORATIONS=party|src/Shop.php"
    "themes|decorations-retro-crt|MY_DECORATIONS=retro-crt|src/Shop.php"
    "themes|decorations-halloween|MY_DECORATIONS=halloween|src/Shop.php"
    # Plan 45's effects (the glitch caught half way), the grid's glowing horizon with the
    # city on it, the Cyberpunk theme with its signs switched on.
    "themes|effect-glitch|MY_DECORATIONS=glitch@panels|src/Shop.php"
    "themes|effect-synthwave-grid|MY_DECORATIONS=synthwave:city@panels|src/Shop.php"
    "themes|effect-neon-rain|MY_DECORATIONS=neon-rain@panels|src/Shop.php"
    "themes|effect-holo-signs|MY_DECORATIONS=holo-signs@panels|src/Shop.php"
    # The theme studio on the Christmas theme, scrolled to its decorations (plan 41c).
    "themes|theme-studio-decorations|THEMED=christmas;STUDIO=1;STUDIO_COMMAND=scroll-to:decorations|src/Shop.php"
    # The Christmas theme's own settings in the studio (plan 41f).
    "themes|studio-options|THEMED=christmas;STUDIO=1;STUDIO_COMMAND=scroll-to:options|src/Shop.php"
    # The studio's work-in-progress theme on the window: Matrix (tinted in its accent's hue)
    # with effects behind the panels — the panels keep their dark background (42g).
    "themes|studio-matrix-live|THEME=matrix;STUDIO=1;STUDIO_LIVE=1;STUDIO_COMMAND=preset:space,category:effects|src/Shop.php"
)
for n in 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14; do
    cases+=("settings|settings-$n|SETTINGS=$n|src/Shop.php")
done

differ=0
count=0
for theme in default-dark default-light; do
    for case in "${cases[@]}"; do
        IFS='|' read -r group name vars file <<<"$case"
        # A group, or one case (`case:<name>`).
        if [[ "$only" == case:* ]]; then
            [[ "$name" != "${only#case:}" ]] && continue
        elif [[ -n "$only" && "$group" != "$only" ]]; then
            continue
        fi
        png="$out/$name-$theme.png"
        target="$png"
        [[ "$mode" == compare ]] && target="$work/$name-$theme.png"
        # Romanian, as the references were made (the app itself starts in English);
        # `SNAPSHOT_LANG=en` for the README's pictures.
        env=(HOME="$work/home" EDITOR_LANG="${SNAPSHOT_LANG:-ro}" EDITOR_SNAPSHOT_THEME="$theme")
        # `SNAPSHOT_SIZE=1600x1000`: a larger window (logical pixels; the README's pictures).
        [[ -n "${SNAPSHOT_SIZE:-}" ]] && env+=(EDITOR_SNAPSHOT_SIZE="$SNAPSHOT_SIZE")
        IFS=';' read -ra assignments <<<"$vars"
        for v in ${assignments[@]+"${assignments[@]}"}; do
            [[ -z "$v" ]] && continue
            [[ "$v" == EDITOR_* ]] && env+=("$v") || env+=("EDITOR_SNAPSHOT_$v")
        done
        count=$((count + 1))
        # A sample put in the project for this case only (the new languages').
        placed=""
        if [[ "$file" != ../* && ! -e "$project/$file" && -f "$work/samples/${file##*/}" ]]; then
            mkdir -p "$(dirname "$project/$file")"
            cp "$work/samples/${file##*/}" "$project/$file"
            placed="$project/$file"
        fi
        # Sample agent sessions of the project (Claude Code's files), this case only: a few
        # titles, each last active at another time.
        if [[ "$vars" == *SESSIONS_SAMPLE=1* ]]; then
            real="$(cd "$project" && pwd -P)"
            sessions="$work/home/.claude/projects/$(printf '%s' "$real" | sed 's/[^A-Za-z0-9]/-/g')"
            mkdir -p "$sessions"
            i=0
            while IFS='|' read -r ago title asked; do
                i=$((i + 1))
                id="0000000$i-1111-4222-8333-44445555666$i"
                printf '{"type":"user","sessionId":"%s","cwd":"%s","timestamp":"2026-11-1%dT09:00:00Z","message":{"role":"user","content":"%s"}}\n{"type":"ai-title","aiTitle":"%s","sessionId":"%s"}\n' \
                    "$id" "$real" "$i" "$asked" "$title" "$id" > "$sessions/$id.jsonl"
                touch -t "$(date -v-"$ago" +%Y%m%d%H%M.%S)" "$sessions/$id.jsonl"
            done <<'SESSIONS'
1M|Add a discount to the cart total|Add a percentage discount to Shop::total
3H|Explain how Cart and Shop work together|How do Cart and Shop work together?
1d|Write tests for Shop::items|Write PHPUnit tests for Shop::items
4d|Rename Cart to Basket|Rename the Cart class to Basket everywhere
12d|Fix the style.css colors in dark mode|The colors in style.css are wrong in dark mode
SESSIONS
        fi
        # The project's own Node tools (46c): their packages in node_modules, this case only.
        if [[ "$vars" == *NODE_TOOLS=1* ]]; then
            mkdir -p "$project/node_modules/eslint/bin" "$project/node_modules/prettier/bin"
            echo '{"name": "eslint", "version": "9.12.0"}' > "$project/node_modules/eslint/package.json"
            echo '{"name": "prettier", "version": "3.3.3"}' > "$project/node_modules/prettier/package.json"
            : > "$project/node_modules/eslint/bin/eslint.js"
            : > "$project/node_modules/prettier/bin/prettier.cjs"
        fi
        # A case in the history's repository (`history:<file>`): that project instead; `-`:
        # nothing opened (the empty window).
        root="$project"
        paths=()
        if [[ "$file" == history:* ]]; then
            root="$history"
            file="${file#history:}"
        fi
        if [[ "$file" != "-" ]]; then
            paths=("$root" "$root/$file")
        fi
        env -u CLICOLOR -u LSCOLORS -u LS_COLORS "${env[@]}" \
            "$bin" --snapshot "$target" ${paths[@]+"${paths[@]}"} >/dev/null 2>&1 \
            || { echo "failed: $name ($theme)" >&2; differ=1; [[ -n "$placed" ]] && rm -f "$placed"; continue; }
        [[ -n "$placed" ]] && rm -f "$placed"
        # What a case put in the project (a SQL file attached, 39n; the extractors' folder,
        # 39o2) gone before the next.
        rm -f "$project/report.sql" "$project/$project_dir/sql-files.toml"
        # And the extractors' folder made by "Go to Extractors Directory" (39o2).
        rm -rf "$project/$project_dir/extractors"
        # And the language server's TypeScript file (43a).
        rm -f "$project/src/cart.ts" "$project/src/card.css" "$project/src/cart.html" "$project/src/shapes.cpp"
        rm -rf "$project/node_modules" "$work/home/.claude"
        if [[ "$mode" == compare ]]; then
            if [[ ! -f "$png" ]]; then
                echo "no reference: $name ($theme)"
                differ=1
            elif ! cmp -s "$png" "$target"; then
                echo "differs: $name ($theme) — $target"
                differ=1
            fi
        fi
    done
done
if [[ "$mode" == record ]]; then
    echo "recorded $count snapshots in $out"
elif [[ $differ == 0 ]]; then
    echo "identical: $count snapshots, both themes"
fi
exit $differ
