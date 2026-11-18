<?php

namespace App\Support;

/**
 * @template TKey of array-key
 * @template TValue
 * @implements \IteratorAggregate<TKey, TValue>
 */
class Collection implements \IteratorAggregate, \Countable
{
    /**
     * @param array<TKey, TValue> $items
     */
    public function __construct(private array $items = [])
    {
    }

    /**
     * @return TValue|null
     */
    public function first(): mixed
    {
        foreach ($this->items as $item) {
            return $item;
        }
        return null;
    }

    /**
     * @template TMap
     * @param callable(TValue, TKey): TMap $callback
     * @return static<TKey, TMap>
     */
    public function map(callable $callback): static
    {
        return new static(array_map($callback, $this->items));
    }

    /**
     * @param callable(TValue): bool $callback
     * @return static<TKey, TValue>
     */
    public function filter(callable $callback): static
    {
        return new static(array_filter($this->items, $callback));
    }

    /**
     * @return array<TKey, TValue>
     */
    public function all(): array
    {
        return $this->items;
    }

    public function count(): int
    {
        return count($this->items);
    }

    /**
     * @return \ArrayIterator<TKey, TValue>
     */
    public function getIterator(): \ArrayIterator
    {
        return new \ArrayIterator($this->items);
    }
}
