<?php

namespace App\Repositories;

/**
 * @template T of object
 */
abstract class Repository
{
    /** @var array<int, T> */
    protected array $items = [];

    /**
     * @return T|null
     */
    public function find(int $id): ?object
    {
        return $this->items[$id] ?? null;
    }

    /**
     * @return list<T>
     */
    public function all(): array
    {
        return array_values($this->items);
    }
}
