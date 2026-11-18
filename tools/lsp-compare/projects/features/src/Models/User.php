<?php

namespace App\Models;

use App\Concerns\HasTimestamps;
use App\Enums\Status;

/**
 * @property string $nickname
 * @method static User findByEmail(string $email)
 * @method bool ban(string $reason)
 */
class User extends Model
{
    use HasTimestamps;

    public function __construct(
        public string $name = '',
        public ?Address $address = null,
        private Status $status = Status::Active,
    ) {
    }

    public function status(): Status
    {
        return $this->status;
    }

    public function rename(string $name): static
    {
        $this->name = $name;
        return $this;
    }

    public function describe(): string
    {
        return 'User ' . $this->name;
    }

    /** @return list<Post> */
    public function posts(): array
    {
        return [];
    }

    /** @param array<int, mixed> $arguments */
    public function __call(string $method, array $arguments): mixed
    {
        return null;
    }

    /** @param array<int, mixed> $arguments */
    public static function __callStatic(string $method, array $arguments): mixed
    {
        return null;
    }

    public function __get(string $name): mixed
    {
        return $this->attributes[$name] ?? null;
    }
}
