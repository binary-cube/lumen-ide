"""The probes of the PHP engines' comparison (plans 43, 47 and 65d): 133 scored questions
(completion 42, definition 29, references 12, rename 9, hover 15, signature 8,
diagnostics 13, noise 5; plan 47g added the callables' strings, plan 65d Symfony's own
names) on three projects: `features` (ours, in projects/features), a
Laravel and a Symfony skeleton (made by run.sh with Composer, plus the probe files in
projects/laravel and projects/symfony).

    python3 -I probes.py <work dir>     writes <work dir>/probes.json

Each probe says where it is asked:
  - `marker`: a `// @Xnn` line of its file, replaced by `insert` while it's asked (the cursor
    at the end of the insert, or at its `|`);
  - `at`: a needle in its file, its `|` being the cursor (`nth`: which occurrence).
and what is expected (see score.py for how each category is scored). Places are given as
(file, needle): the line (0-based) of the needle's first occurrence in that file.
"""

import json
import sys
from pathlib import Path

PROBES = []


def place(file, needle, nth=0):
    return {"file": file, "needle": needle, "nth": nth}


def probe(id, category, project, file, **fields):
    PROBES.append({"id": id, "category": category, "project": project, "file": file, **fields})


# ─── completion (37): every expected name offered (`partial`: some) ───

F = "src/Probes.php"


def completion(id, insert, expect, project="features", file=F):
    probe(id, "completion", project, file, marker=f"// @{id}", insert=insert, expect=expect)


completion("C01", "$user->", ["name", "address", "rename", "status", "describe"])
completion("C02", "$this->shop->cart()->", ["add", "lines", "total"])
completion("C03", "$user->rename('x')->", ["status", "describe", "rename"])
completion("C04", "$users->first()->", ["name", "rename"])
completion("C05", "$shop->customer()?->", ["name", "address"])
completion("C06", "$line->", ["sku", "qty", "subtotal"])
completion("C07", "Status::", ["Active", "Banned", "cases", "from", "tryFrom"])
completion("C08", "$user->status()->", ["label", "name", "value"])
completion("C09", "Status::from('active')->", ["label", "value"])
completion("C10", "$user->nick", ["nickname"])
completion("C11", "$repo->find(1)->", ["name", "rename"])
completion("C12", "$user->address->", ["city", "street", "format"])
completion("C13", "$d->", ["modify", "format", "getTimestamp"])
completion("C14", "$uri->", ["getHost", "getScheme"])
completion("C15", "User::", ["create", "findByEmail"])
completion("C16", "$this->", ["shop", "cartTotal", "members"])
completion("C17", "format_n", ["format_name"])
completion("C18", "new Sho", ["Shop"])
completion("C19", "str_repl", ["str_replace"])
completion("C20", "use App\\Mo", ["Models"])
completion("C21", "$eith", ["either"])
completion("C22", "Cart::", ["MAX_LINES"])
completion("C23", "$users->map(fn ($u) => $u->", ["name", "rename"])
completion("C24", "$user->ba", ["ban"])
completion("C25", "$user->tou", ["touch", "touchedAt"])
completion("C26", "$row['user']->", ["name", "rename"])
completion("C27", "$thing->", ["add", "total"])
completion("C28", "make(Cart::class)->", ["add", "total"])
completion("C29", "$either->", ["total", "checkout"])
completion("C30", "User::create([])->", ["rename", "describe"])
L = "app/LaravelProbe.php"
completion("C31", "User::where('email', 'a')->", ["first", "get", "orderBy"], "laravel", L)
completion("C32", "User::query()->", ["where", "first"], "laravel", L)
completion("C33", "Cache::", ["get", "put", "remember"], "laravel", L)
completion("C34", "collect([1, 2])->", ["map", "filter", "first"], "laravel", L)
completion("C35", "Str::", ["slug", "camel"], "laravel", L)
Y = "src/Controller/ProbeController.php"
completion("C36", "$request->query->", ["get", "all", "has"], "symfony", Y)
completion("C37", "$this->", ["render", "json", "redirectToRoute"], "symfony", Y)

# ─── definition (28): the first place given is the declaration (`partial`: another one) ───


def definition(id, at, expect, project="features", file=F, nth=0):
    probe(id, "definition", project, file, at=at, nth=nth, expect=expect)


definition("D01", "$shop = new Sh|op($cart);", place("src/Services/Shop.php", "final class Shop"))
definition("D02", "$cart->a|dd('sku-1'", place("src/Services/Cart.php", "public function add("))
definition("D03", "$owner = $user->na|me;", place("src/Models/User.php", "public string $name"))
definition(
    "D04",
    "$sum = $this->shop->cart()->to|tal();",
    place("src/Services/Cart.php", "public function total()"),
)
definition(
    "D05", "$formatted = format_na|me(", place("src/Support/helpers.php", "function format_name(")
)
definition("D06", "$city = DEFAULT_CI|TY;", place("src/Support/helpers.php", "const DEFAULT_CITY"))
definition("D07", "$banned = Status::Bann|ed;", place("src/Enums/Status.php", "case Banned"))
definition(
    "D08",
    "$made = User::crea|te(",
    place("src/Models/Model.php", "public static function create("),
)
definition(
    "D09",
    "implements Describ|able",
    place("src/Contracts/Describable.php", "interface Describable"),
    file="src/Models/Model.php",
)
definition(
    "D10",
    "use HasTimest|amps;",
    place("src/Concerns/HasTimestamps.php", "trait HasTimestamps"),
    file="src/Models/User.php",
)
definition("D11", "$user->sa|ve();", place("src/Models/Model.php", "public function save()"))
definition(
    "D12", "$nick = $user->nickn|ame;", place("src/Models/User.php", "@property string $nickname")
)
definition(
    "D13",
    "$users->first()->ren|ame('Ion')",
    place("src/Models/User.php", "public function rename("),
)
definition("D14", "return $label . $own|er", place(F, "$owner = $user->name;"))
definition(
    "D15",
    "return $lab|el .",
    place(F, "public function navigation(User $user, Cart $cart, Collection $users, string $label)"),
)
definition(
    "D16",
    "User::whe|re('email'",
    place("vendor/laravel/framework/src/Illuminate/Database/Eloquent/Builder.php",
          "public function where("),
    "laravel",
    L,
)
definition("D17", "$user->b|an('spam')", place("src/Models/User.php", "@method bool ban("))
definition("D18", "$max = Cart::MAX_LI|NES;", place("src/Services/Cart.php", "public const MAX_LINES"))
definition(
    "D19",
    "self::MAX_LI|NES",
    place("src/Services/Cart.php", "public const MAX_LINES"),
    file="src/Services/Cart.php",
)
definition("D20", "use App\\Models\\Addr|ess;", place("src/Models/Address.php", "class Address"))
definition(
    "D21",
    "\\App\\Models\\Us|er::class",
    place("app/Models/User.php", "class User"),
    "laravel",
    L,
)
definition(
    "D22",
    "$url = rou|te('home')",
    place("vendor/laravel/framework/src/Illuminate/Foundation/helpers.php", "function route("),
    "laravel",
    L,
)
definition(
    "D23",
    "$this->ren|der(",
    place("vendor/symfony/framework-bundle/Controller/AbstractController.php",
          "protected function render("),
    "symfony",
    Y,
)
definition(
    "D24",
    "new Respo|nse('Page '",
    place("vendor/symfony/http-foundation/Response.php", "class Response"),
    "symfony",
    Y,
)
definition(
    "D25",
    "$user->status()->lab|el();",
    place("src/Enums/Status.php", "public function label()"),
)

# Methods named in callables' strings (47g).
CUSTOMER = place("src/Services/Shop.php", "public function customer()")
definition("D26", "$byObject = [$shop, 'custo|mer'];", CUSTOMER)
definition("D27", "$byClass = [Shop::class, 'custo|mer'];", CUSTOMER)
definition("D28", "Shop::custo|mer';", CUSTOMER)

# ─── references (11) / rename (8): exactly the lines expected (`partial`: some of them) ───


def references(id, at, expect, file=F, nth=0, project="features"):
    probe(id, "references", project, file, at=at, nth=nth, expect=expect)


def rename(id, at, expect, file=F, nth=0, project="features"):
    probe(id, "rename", project, file, at=at, nth=nth, expect=expect)


SUBTOTAL = [
    place("src/Services/LineItem.php", "public function subtotal()"),
    place("src/Services/Cart.php", "$sum += $line->subtotal();"),
    place(F, "$text .= $line->sku . $line->subtotal();"),
]
FORMAT_NAME = [
    place("src/Support/helpers.php", "function format_name("),
    place(F, "use function App\\Support\\format_name;"),
    place(F, "$formatted = format_name($user, true);"),
    place(F, "$text = str_replace('a', 'b', format_name($user));"),
    place("src/Problems.php", "use function App\\Support\\format_name;"),
    place("src/Problems.php", "    format_name();"),
]
ADDRESS = [
    place("src/Models/Address.php", "class Address"),
    place("src/Models/User.php", "public ?Address $address = null,"),
    place(F, "use App\\Models\\Address;"),
    place(F, "$home = new Address(DEFAULT_CITY, 'Main');"),
]
references("R01", "$line->subto|tal();", SUBTOTAL)
references("R02", "function format_na|me(", FORMAT_NAME, file="src/Support/helpers.php")
references(
    "R03",
    "$home->ci|ty;",
    [
        place("src/Models/Address.php", "public string $city = '',"),
        place("src/Models/Address.php", "return $this->street . ', ' . $this->city;"),
        place(F, "$home->city;"),
    ],
)
references(
    "R04",
    "$city = DEFAULT_CI|TY;",
    [
        place("src/Support/helpers.php", "const DEFAULT_CITY"),
        place(F, "use const App\\Support\\DEFAULT_CITY;"),
        place(F, "$city = DEFAULT_CITY;"),
        place(F, "$home = new Address(DEFAULT_CITY, 'Main');"),
    ],
)
references(
    "R05",
    "$max = Cart::MAX_LI|NES;",
    [
        place("src/Services/Cart.php", "public const MAX_LINES"),
        place("src/Services/Cart.php", "self::MAX_LINES"),
        place(F, "$max = Cart::MAX_LINES;"),
    ],
)
references(
    "R06",
    "return $this->shop->ca|rt()->total();",
    [
        place("src/Services/Shop.php", "public function cart(): Cart"),
        place("src/Services/Shop.php", "$amount = $this->cart()->total();"),
        place(F, "return $this->shop->cart()->total();"),
        place(F, "$sum = $this->shop->cart()->total();"),
    ],
)
references(
    "R07",
    "case Bann|ed",
    [place("src/Enums/Status.php", "case Banned"), place(F, "$banned = Status::Banned;")],
    file="src/Enums/Status.php",
)
references("R08", "$home = new Addre|ss(", ADDRESS)
references(
    "R09",
    "return $cart->to|tal() +",
    [
        place("src/Services/Cart.php", "public function total()"),
        place("src/Services/Shop.php", "$amount = $this->cart()->total();"),
        place(F, "return $this->shop->cart()->total();"),
        place(F, "$sum = $this->shop->cart()->total();"),
        place(F, "return $cart->total() +"),
    ],
)
references(
    "R10",
    "$made = User::cre|ate(",
    [
        place("src/Models/Model.php", "public static function create("),
        place(F, "$made = User::create(['name' => 'Ana']);"),
    ],
)

CUSTOMER_USES = [
    CUSTOMER,
    place(F, "$byObject = [$shop, 'customer'];"),
    place(F, "$byClass = [Shop::class, 'customer'];"),
    place(F, "$byName = 'App\\Services\\Shop::customer';"),
    place(F, "$closure = \\Closure::fromCallable([$shop, 'customer']);"),
    place(F, "$exists = method_exists($shop, 'customer');"),
    place(F, "$shop->customer()];"),
]
references("R11", "public function custo|mer()", CUSTOMER_USES, file="src/Services/Shop.php")

rename(
    "N01",
    "$to|tal = $sum + $max;",
    [
        place(F, "$total = $sum + $max;"),
        place(F, "$total = $total * 2;"),
        place(F, "return $label . $owner"),
    ],
)
rename(
    "N02",
    "function check|out(",
    [
        place("src/Services/Shop.php", "public function checkout("),
        place(F, "$shop->checkout('EUR');"),
    ],
    file="src/Services/Shop.php",
)
rename("N03", "$formatted = format_na|me(", FORMAT_NAME)
rename(
    "N04",
    "public function descr|ibe(): string",
    [
        place("src/Contracts/Describable.php", "public function describe(): string;"),
        place("src/Models/Model.php", "abstract public function describe(): string;"),
        place("src/Models/User.php", "public function describe(): string"),
        place(F, "$user->describe();"),
    ],
    file="src/Models/User.php",
)
rename(
    "N05",
    "public int $q|ty,",
    [
        place("src/Services/LineItem.php", "public int $qty,"),
        place("src/Services/LineItem.php", "return $this->qty * $this->price;"),
    ],
    file="src/Services/LineItem.php",
)
rename("N06", "class Addr|ess", ADDRESS, file="src/Models/Address.php")
rename(
    "N07",
    "$curr|ency !== ''",
    [
        place("src/Services/Shop.php", "public function checkout(string $currency"),
        place("src/Services/Shop.php", "$currency !== ''"),
    ],
    file="src/Services/Shop.php",
)

rename("N08", "$byObject = [$shop, 'custo|mer'];", CUSTOMER_USES)

# ─── hover (14): every expected text in the card (`partial`: some) ───


def hover(id, at, expect, project="features", file=F, nth=0):
    probe(id, "hover", project, file, at=at, nth=nth, expect=expect)


hover("H01", "$shop = new Sh|op($cart);", ["class", "Shop"])
hover("H02", "$first = $use|rs->first()", ["Collection", "User"])
hover("H03", "$text .= $li|ne->sku", ["LineItem"])
hover("H04", "$cart->a|dd('sku-1'", ["add", "string $sku", "int $qty"])
hover("H05", "$formatted = format_na|me(", ["format_name", "User $user"])
hover("H06", "$text = str_repl|ace(", ["str_replace", "$search"])
hover("H07", "$next = $d->modi|fy(", ["modify", "string $modifier"])
hover("H08", "$first = Status::Act|ive;", ["Active", "Status"])
hover("H09", "$owner = $user->na|me;", ["string", "$name"])
hover("H10", "(new Legacy())->o|ld();", ["deprecated"], file=F)
hover("H11", "$user->b|an('spam')", ["ban", "string $reason"])
hover("H12", "$max = Cart::MAX_LI|NES;", ["MAX_LINES", "50"])
hover("H13", "$total = $s|um + $max;", ["float"])
hover("H14", "$fou|nd = User::where(", ["User"], "laravel", L)

# ─── signature (8): the parameters expected (servers may leave the name out), the right
# argument active ───


def signature(id, insert, expect, active):
    # As an editor has it: the parenthesis closed when it was typed, the cursor before it.
    probe(id, "signature", "features", F, marker=f"// @{id}", insert=insert + "|);",
          expect=expect, active=active)


signature("S01", "format_name($user, ", ["User $user", "bool $upper"], 1)
signature("S02", "$cart->add('sku', ", ["string $sku", "int $qty"], 1)
signature("S03", "str_replace('a', ", ["$search", "$replace"], 1)
signature("S04", "new Shop(", ["Cart $cart"], 0)
signature("S05", "User::create(", ["array $attributes"], 0)
signature("S06", "$shop->checkout('EUR', ", ["string $currency", "bool $notify"], 1)
signature("S07", "$users->map(", ["callable"], 0)
signature("S08", "$user->ban(", ["string $reason"], 0)

# ─── diagnostics (10): a problem on the line, its message matching ───

P = "src/Problems.php"
ARGUMENTS = r"argument|parameter|expect"
DEPRECATED = r"deprecat|depreciat|învechit"


def diagnostic(id, needle, pattern, project="features", file=P):
    probe(id, "diagnostics", project, file, expect=place(file, needle), pattern=pattern)


diagnostic("G01", "$cart->checkoutNow();", r"checkoutNow")
diagnostic("G02", "new UnknownThing();", r"UnknownThing")
diagnostic("G03", "    format_name();", ARGUMENTS)
diagnostic("G04", "echo str_replace('a', 'b');", ARGUMENTS)
diagnostic("G05", "echo strlen('a', 'b');", ARGUMENTS)
diagnostic("G06", "(new Legacy())->old();", DEPRECATED)
diagnostic("G07", "echo utf8_encode('x');", DEPRECATED)
diagnostic("G08", "echo old_format();", DEPRECATED)
diagnostic("G09", "$unused = 42;", r"unused|not used|never used|neutilizat|nefolosit|nu e folosit")
diagnostic("G10", "$cart->add();", ARGUMENTS)

# ─── noise (5): no error or warning in a file that has none ───


def noise(id, project, file):
    probe(id, "noise", project, file)


noise("Z01", "features", "src/Services/Shop.php")
noise("Z02", "features", "src/Support/Collection.php")
noise("Z03", "laravel", "app/Models/User.php")
noise("Z04", "laravel", L)
noise("Z05", "symfony", Y)


# ─── Symfony's own names (plan 65d): routes, services, parameters, translations ───
# Asked of the Symfony skeleton, its container compiled by run.sh (`cache:warmup`): the
# framework module's answers, beside the language's (the other servers know none of them).

completion("C38", "$this->redirectToRoute('|');", ["probe", "probe_view"], "symfony", Y)
completion(
    "C39",
    "$this->container->get(\\Symfony\\Component\\HttpFoundation\\RequestStack::class)->",
    ["getCurrentRequest", "getMainRequest"],
    "symfony",
    Y,
)
completion("C40", "$this->getParameter('kernel.|');", ["kernel.project_dir", "kernel.debug"],
           "symfony", Y)
completion("C41", "$this->container->get('|');", ["router", "request_stack"], "symfony", Y)
completion("C42", "t('probe.|');", ["probe.title"], "symfony", Y)
ROUTE = [place(Y, "name: 'probe')"), place(Y, "redirectToRoute('probe')")]
definition("D29", "redirectToRoute('pro|be')", place(Y, "name: 'probe')"), "symfony", Y)
references("R12", "redirectToRoute('pro|be')", ROUTE, Y, project="symfony")
rename("N09", "redirectToRoute('pro|be')", ROUTE, Y, project="symfony")
hover("H15", "redirectToRoute('pro|be')", ["route probe", "/probe"], "symfony", Y)
SP = "src/Controller/SymfonyProblems.php"
diagnostic("G11", "redirectToRoute('nowhere')", r"nowhere", "symfony", SP)
diagnostic("G12", "get('no.such.service')", r"no\.such\.service", "symfony", SP)
diagnostic("G13", "t('probe.missing')", r"probe\.missing", "symfony", SP)


# ─── Prepared: the texts, the offsets, the lines ───


def line_of(text, offset):
    return text.count("\n", 0, offset)


def find(text, needle, nth, what):
    at = -1
    for _ in range(nth + 1):
        at = text.find(needle, at + 1)
        if at < 0:
            raise SystemExit(f"{what}: `{needle}` not found")
    return at


def prepare(work):
    roots = {name: str(work / "projects" / name) for name in ("features", "laravel", "symfony")}
    texts = {}

    def read(project, file):
        key = (project, file)
        if key not in texts:
            texts[key] = (Path(roots[project]) / file).read_text(encoding="utf-8")
        return texts[key]

    def resolve(project, p):
        text = read(project, p["file"])
        return [p["file"], line_of(text, find(text, p["needle"], p["nth"], p["file"]))]

    prepared = []
    for p in PROBES:
        text = read(p["project"], p["file"])
        out = {k: v for k, v in p.items() if k not in ("expect", "at", "marker", "insert", "nth")}
        if "marker" in p:
            insert = p["insert"]
            cursor = insert.index("|") if "|" in insert else len(insert)
            insert = insert.replace("|", "")
            at = find(text, p["marker"], 0, p["id"])
            text = text[:at] + insert + text[at + len(p["marker"]):]
            out["text"] = text
            out["offset"] = at + cursor
        elif "at" in p:
            needle = p["at"]
            cursor = needle.index("|")
            at = find(text, needle.replace("|", ""), p.get("nth", 0), p["id"])
            out["offset"] = at + cursor
        if "offset" in out:
            before = text[: out["offset"]]
            out["line"] = before.count("\n")
            # LSP's character: UTF-16 code units since the line's start.
            column = before[before.rfind("\n") + 1:]
            out["character"] = len(column.encode("utf-16-le")) // 2
            # The cursor as a byte offset (the app counts bytes).
            out["offset"] = len(before.encode("utf-8"))
        expect = p.get("expect")
        if isinstance(expect, dict):
            out["expect"] = resolve(p["project"], expect)
        elif isinstance(expect, list) and expect and isinstance(expect[0], dict):
            out["expect"] = sorted(resolve(p["project"], e) for e in expect)
        elif expect is not None:
            out["expect"] = expect
        prepared.append(out)
    counts = {}
    for p in prepared:
        counts[p["category"]] = counts.get(p["category"], 0) + 1
    return {"projects": roots, "probes": prepared, "counts": counts}


if __name__ == "__main__":
    work = Path(sys.argv[1]).resolve()
    data = prepare(work)
    (work / "probes.json").write_text(json.dumps(data, indent=1), encoding="utf-8")
    print(f"{len(data['probes'])} probes: {data['counts']}")
