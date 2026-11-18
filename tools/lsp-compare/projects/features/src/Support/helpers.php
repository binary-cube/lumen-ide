<?php

namespace App\Support;

use App\Models\User;

const DEFAULT_CITY = 'Cluj';

function format_name(User $user, bool $upper = false): string
{
    return $upper ? strtoupper($user->name) : $user->name;
}

/**
 * @template T of object
 * @param class-string<T> $class
 * @return T
 */
function make(string $class): object
{
    return new $class();
}
