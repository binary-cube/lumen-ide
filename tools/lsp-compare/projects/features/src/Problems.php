<?php

namespace App;

use App\Models\User;
use App\Services\Cart;

use function App\Support\format_name;

/**
 * What each engine should flag: one problem per line.
 */
function problems(User $user, Cart $cart): void
{
    $cart->checkoutNow();
    $thing = new UnknownThing();
    format_name();
    echo str_replace('a', 'b');
    echo strlen('a', 'b');
    (new Legacy())->old();
    echo utf8_encode('x');
    echo old_format();
    $unused = 42;
    $cart->add();
    echo $thing::class;
}
