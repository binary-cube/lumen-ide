<?php

namespace App\Repositories;

use App\Models\User;

/**
 * @extends Repository<User>
 */
final class UserRepository extends Repository
{
    public function byName(string $name): ?User
    {
        foreach ($this->all() as $user) {
            if ($user->name === $name) {
                return $user;
            }
        }
        return null;
    }
}
