<?php

namespace App\Services;

use App\Models\User;

final class Shop
{
    public function __construct(
        private Cart $cart,
        private ?User $customer = null,
    ) {
    }

    public function cart(): Cart
    {
        return $this->cart;
    }

    public function customer(): ?User
    {
        return $this->customer;
    }

    public function checkout(string $currency, bool $notify = true): float
    {
        $amount = $this->cart()->total();
        if ($notify && $currency !== '') {
            $amount = round($amount, 2);
        }
        return $amount;
    }
}
