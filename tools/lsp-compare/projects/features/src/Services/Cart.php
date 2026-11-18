<?php

namespace App\Services;

final class Cart
{
    public const MAX_LINES = 50;

    /** @var list<LineItem> */
    private array $lines = [];

    public function add(string $sku, int $qty = 1, float $price = 0.0): self
    {
        if (count($this->lines) < self::MAX_LINES) {
            $this->lines[] = new LineItem($sku, $qty, $price);
        }
        return $this;
    }

    /** @return list<LineItem> */
    public function lines(): array
    {
        return $this->lines;
    }

    public function total(): float
    {
        $sum = 0.0;
        foreach ($this->lines as $line) {
            $sum += $line->subtotal();
        }
        return $sum;
    }
}
