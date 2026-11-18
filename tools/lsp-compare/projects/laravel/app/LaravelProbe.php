<?php

namespace App;

use App\Models\User;
use Illuminate\Support\Facades\Cache;
use Illuminate\Support\Str;

/**
 * The probes' code in a Laravel project: each `// @Cnn` line is replaced by a probe's text
 * while it's asked; the rest is asked where it stands.
 */
final class LaravelProbe
{
    public function completion(): void
    {
        // @C31
        // @C32
        // @C33
        // @C34
        // @C35
    }

    public function navigation(): string
    {
        $found = User::where('email', 'ana@example.com')->first();
        $model = \App\Models\User::class;
        $url = route('home');
        return $model . $url . ($found?->name ?? '');
    }

    public function cached(): mixed
    {
        return Cache::remember('users', 60, fn () => Str::slug('Ana Pop'));
    }
}
