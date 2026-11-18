<?php

namespace App\Services;

final class LineItem
{
    public function __construct(
        public string $sku,
        public int $qty,
        public float $price,
    ) {
    }

    public function subtotal(): float
    {
        return $this->qty * $this->price;
    }
}
