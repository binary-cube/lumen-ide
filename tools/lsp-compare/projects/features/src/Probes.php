<?php

namespace App;

use App\Enums\Status;
use App\Models\Address;
use App\Models\User;
use App\Repositories\UserRepository;
use App\Services\Cart;
use App\Services\Shop;
use App\Support\Collection;

use function App\Support\format_name;
use function App\Support\make;

use const App\Support\DEFAULT_CITY;

// @C20

/**
 * The probes' code: each `// @Cnn` / `// @Snn` line is replaced by a probe's text while it's
 * asked (completion, signature help); the rest is asked where it stands.
 */
final class Probes
{
    public function __construct(
        private Shop $shop,
    ) {
    }

    public function cartTotal(): float
    {
        return $this->shop->cart()->total();
    }

    public function members(User $user, UserRepository $repo, object $thing, bool $flag): void
    {
        // @C01
        // @C03
        // @C08
        // @C10
        // @C11
        // @C12
        // @C24
        // @C25
        // @C30
        if ($thing instanceof Cart) {
            // @C27
        }
        $either = $flag ? new Cart() : new Shop(new Cart());
        // @C29
        // @C21
    }

    /**
     * @param Collection<int, User> $users
     */
    public function generics(Collection $users, Cart $cart, Shop $shop): void
    {
        // @C04
        // @C05
        foreach ($cart->lines() as $line) {
            // @C06
        }
        // @C23
        // @C28
        /** @var array{id: int, user: User} $row */
        $row = ['id' => 1, 'user' => new User()];
        // @C26
    }

    public function language(): void
    {
        // @C07
        // @C09
        $d = new \DateTimeImmutable();
        // @C13
        $uri = \Uri\Rfc3986\Uri::parse('https://example.com/');
        // @C14
        // @C15
        // @C16
        // @C17
        // @C18
        // @C19
        // @C22
        // @C02
    }

    public function signatures(User $user, Cart $cart, Shop $shop): void
    {
        /** @var Collection<int, User> $users */
        $users = new Collection([$user]);
        // @S01
        // @S02
        // @S03
        // @S04
        // @S05
        // @S06
        // @S07
        // @S08
    }

    /**
     * @param Collection<int, User> $users
     */
    public function navigation(User $user, Cart $cart, Collection $users, string $label): string
    {
        $shop = new Shop($cart);
        $cart->add('sku-1', 2, 9.5);
        $owner = $user->name;
        $sum = $this->shop->cart()->total();
        $formatted = format_name($user, true);
        $city = DEFAULT_CITY;
        $banned = Status::Banned;
        $made = User::create(['name' => 'Ana']);
        $user->save();
        $nick = $user->nickname;
        $first = $users->first()->rename('Ion');
        $user->ban('spam');
        $max = Cart::MAX_LINES;
        $home = new Address(DEFAULT_CITY, 'Main');
        $text = $user->status()->label();
        $total = $sum + $max;
        $total = $total * 2;
        return $label . $owner . $formatted . $city . $nick . $text . $total . $home->city;
    }

    /**
     * @return list<mixed>
     */
    public function callables(Shop $shop, mixed $mystery): array
    {
        $byObject = [$shop, 'customer'];
        $byClass = [Shop::class, 'customer'];
        $byName = 'App\Services\Shop::customer';
        $closure = \Closure::fromCallable([$shop, 'customer']);
        $exists = method_exists($shop, 'customer');
        $unsure = [$mystery, 'customer'];
        return [$byObject, $byClass, $byName, $closure, $exists, $unsure, $shop->customer()];
    }

    public function more(User $user, Cart $cart, Shop $shop): float
    {
        $d = new \DateTimeImmutable();
        $next = $d->modify('+1 day');
        $text = str_replace('a', 'b', format_name($user));
        $first = Status::Active;
        foreach ($cart->lines() as $line) {
            $text .= $line->sku . $line->subtotal();
        }
        (new Legacy())->old();
        $shop->checkout('EUR');
        $user->describe();
        // The cart's total() is mentioned here, and in the string below: neither is a use.
        $note = 'total() of the cart';
        return $cart->total() + strlen($text . $note) + $next->getTimestamp();
    }
}
